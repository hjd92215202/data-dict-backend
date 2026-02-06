use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};
use dotenvy::dotenv;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use jieba_rs::Jieba;
use once_cell::sync::Lazy;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use rand::rngs::OsRng;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// 声明子模块
mod handlers;
mod middleware;
mod models;
mod services;

// 使用 Lazy 确保 Jieba 词库只在启动时加载一次，并全局可用
pub static JIEBA: Lazy<RwLock<Jieba>> = Lazy::new(|| RwLock::new(Jieba::new()));

// 定义全局状态
pub struct AppState {
    pub db: PgPool,
    pub qdrant: Qdrant,
    pub embed_model: Mutex<TextEmbedding>,
}

/// 确保数据库中存在默认管理员 admin/admin
async fn ensure_default_admin(pool: &PgPool) {
    let username = "admin";
    let user_exists = sqlx::query!("SELECT id FROM users WHERE username = $1", username)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

    if user_exists.is_none() {
        tracing::info!("未检测到管理员账号，正在创建默认账号: admin/admin");
        let password = "admin";
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .expect("无法生成密码哈希");

        let _ = sqlx::query!(
            "INSERT INTO users (username, password_hash, role) VALUES ($1, $2, $3)",
            username,
            password_hash,
            "admin"
        )
        .execute(pool)
        .await;
        tracing::info!("默认管理员账号创建完毕");
    }
}

/// 同步词根向量到 Qdrant
async fn sync_roots_to_qdrant(state: &AppState) {
    tracing::info!("正在同步 [标准词根] 向量到 Qdrant...");
    let roots = sqlx::query_as!(
        crate::models::word_root::WordRoot,
        "SELECT id, cn_name, en_abbr, en_full_name, associated_terms, remark, created_at FROM standard_word_roots"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    if roots.is_empty() {
        return;
    }

    let mut points = Vec::new();
    let mut model = state.embed_model.lock().await;

    for root in &roots {
        // 增强向量特征：中文名 + 英文全称 + 同义词
        let text = format!(
            "{} {} {}",
            root.cn_name,
            root.en_full_name.as_deref().unwrap_or(""),
            root.associated_terms.as_deref().unwrap_or("")
        );

        if let Ok(embeddings) = model.embed(vec![text], None) {
            let mut payload: std::collections::HashMap<String, qdrant_client::qdrant::Value> =
                std::collections::HashMap::new();
            payload.insert("cn_name".to_string(), root.cn_name.clone().into());
            payload.insert("en_abbr".to_string(), root.en_abbr.clone().into());

            points.push(PointStruct::new(
                root.id as u64,
                embeddings[0].clone(),
                payload,
            ));
        }
    }

    if !points.is_empty() {
        let _ = state
            .qdrant
            .upsert_points(UpsertPointsBuilder::new("word_roots", points))
            .await;
        tracing::info!("完成 {} 条 [词根] 向量同步", roots.len());
    }
}

/// 同步标准字段向量到 Qdrant (用于用户端模糊/语义搜索)
async fn sync_fields_to_qdrant(state: &AppState) {
    tracing::info!("正在同步 [标准字段] 向量到 Qdrant...");
    let fields = sqlx::query_as!(
        crate::models::field::StandardField,
        r#"SELECT id, field_cn_name, field_en_name, composition_ids as "composition_ids!", 
           data_type, associated_terms, is_standard as "is_standard!", created_at FROM standard_fields"#
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    if fields.is_empty() {
        return;
    }

    let mut points = Vec::new();
    let mut model = state.embed_model.lock().await;

    for field in &fields {
        // 向量特征：标准中文名 + 关联词
        let text = format!(
            "{} {}",
            field.field_cn_name,
            field.associated_terms.as_deref().unwrap_or("")
        );

        if let Ok(embeddings) = model.embed(vec![text], None) {
            let mut payload: std::collections::HashMap<String, qdrant_client::qdrant::Value> =
                std::collections::HashMap::new();
            payload.insert("cn_name".to_string(), field.field_cn_name.clone().into());
            payload.insert("en_name".to_string(), field.field_en_name.clone().into());

            points.push(PointStruct::new(
                field.id as u64,
                embeddings[0].clone(),
                payload,
            ));
        }
    }

    if !points.is_empty() {
        let _ = state
            .qdrant
            .upsert_points(UpsertPointsBuilder::new("standard_fields", points))
            .await;
        tracing::info!("完成 {} 条 [标准字段] 向量同步", fields.len());
    }
}

/// 初始化 Qdrant 两个独立的集合
async fn init_qdrant_collections(qdrant: &Qdrant) {
    let collections = vec!["word_roots", "standard_fields"];
    for name in collections {
        if !qdrant.collection_exists(name).await.unwrap_or(false) {
            qdrant
                .create_collection(
                    CreateCollectionBuilder::new(name)
                        .vectors_config(VectorParamsBuilder::new(384, Distance::Cosine)),
                )
                .await
                .expect(&format!("无法创建 Qdrant 集合: {}", name));
        }
    }
}

async fn init_custom_dictionary(pool: &PgPool) {
    tracing::info!("正在加载标准词根词典...");
    let roots = sqlx::query!("SELECT cn_name FROM standard_word_roots")
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    let mut jieba_write = JIEBA.write().await;
    for r in &roots {
        jieba_write.add_word(&r.cn_name, Some(99999), None);
    }
    tracing::info!("自定义词典加载完成，共计 {} 个词条", roots.len());
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    // 1. 日志初始化
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        "日志系统初始化完成, 当前级别: {}",
        std::env::var("RUST_LOG").unwrap_or_default()
    );

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    // 2. 数据库连接池
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("Failed to create database connection pool");

    // 3. 执行启动初始化
    ensure_default_admin(&pool).await;
    init_custom_dictionary(&pool).await;

    // 4. 获取模型缓存路径并初始化 Embedding 模型
    let current_dir = env::current_dir().expect("Failed to get current dir");
    let cache_path = current_dir.join("model").join("fastembed_cache");

    let qdrant = Qdrant::from_url("http://localhost:6334").build().unwrap();
    init_qdrant_collections(&qdrant).await;

    let model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::ParaphraseMLMiniLML12V2)
            .with_cache_dir(cache_path)
            .with_show_download_progress(false),
    )
    .expect("Failed to load embedding model");

    let shared_state = Arc::new(AppState {
        db: pool,
        qdrant,
        embed_model: Mutex::new(model),
    });

    // 5. 启动同步
    sync_roots_to_qdrant(&shared_state).await;
    sync_fields_to_qdrant(&shared_state).await;

    // 6. 配置 CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 7. 路由聚合
    let auth_routes = Router::new()
        .route("/signup", post(handlers::auth_handler::signup))
        .route("/login", post(handlers::auth_handler::login));

    let public_routes = Router::new()
        .route("/search", get(handlers::field_handler::search_field))
        .route(
            "/similar-roots",
            get(handlers::mapping_handler::search_similar_roots),
        );

    let admin_routes = Router::new()
        .route(
            "/roots",
            post(handlers::word_root_handler::create_root)
                .get(handlers::word_root_handler::list_roots),
        )
        .route(
            "/roots/batch",
            post(handlers::word_root_handler::batch_create_roots),
        )
        .route(
            "/roots/:id",
            put(handlers::word_root_handler::update_root)
                .delete(handlers::word_root_handler::delete_root),
        )
        .route(
            "/fields",
            post(handlers::field_handler::create_field).get(handlers::field_handler::list_fields),
        )
        .route(
            "/fields/clear",
            delete(handlers::field_handler::clear_all_fields),
        )
        .route(
            "/fields/:id",
            get(handlers::field_handler::get_field_details)
                .put(handlers::field_handler::update_field)
                .delete(handlers::field_handler::delete_field),
        )
        .route(
            "/roots/clear",
            delete(handlers::word_root_handler::clear_all_roots),
        )
        .route(
            "/users",
            post(handlers::auth_handler::create_user_admin).get(handlers::auth_handler::list_users),
        )
        .route(
            "/users/:id",
            put(handlers::auth_handler::update_user_role)
                .delete(handlers::auth_handler::delete_user),
        )
        .route("/suggest", get(handlers::mapping_handler::suggest_mapping))
        .layer(axum::middleware::from_fn_with_state(
            shared_state.clone(),
            middleware::auth::guard,
        ));

    let app = Router::new()
        .nest("/api/auth", auth_routes)
        .nest("/api/public", public_routes)
        .nest("/api/admin", admin_routes)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024)) // 允许 10MB 的请求体
        .with_state(shared_state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("🚀 Server started at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
