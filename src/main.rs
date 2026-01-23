use axum::{
    routing::{get, post, put},
    Router,
};
use dotenvy::dotenv;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use jieba_rs::Jieba;
use once_cell::sync::Lazy;
use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};
use qdrant_client::Qdrant;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::net::SocketAddr;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::{RwLock, Mutex};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::env;

// 声明子模块
mod handlers;
mod middleware;
mod models;
mod services;

// 使用 Lazy 确保 Jieba 词库只在启动时加载一次，并全局可用
pub static JIEBA: Lazy<RwLock<Jieba>> = Lazy::new(|| RwLock::new(Jieba::new()));

// 定义全局状态，方便在 Handler 中获取数据库连接池
pub struct AppState {
    pub db: PgPool,
    pub qdrant: Qdrant,
    pub embed_model:  Mutex<TextEmbedding>,
}

async fn init_qdrant_collection(qdrant: &Qdrant) {
    let collection_name = "word_roots";
    // 如果集合不存在则创建
    if !qdrant
        .collection_exists(collection_name)
        .await
        .unwrap_or(false)
    {
        qdrant
            .create_collection(
                CreateCollectionBuilder::new(collection_name)
                    .vectors_config(VectorParamsBuilder::new(384, Distance::Cosine)), // MiniLM 模型维度为 384
            )
            .await
            .expect("无法创建 Qdrant 集合");
    }
}

async fn init_custom_dictionary(pool: &PgPool) {
    tracing::info!("正在加载自定义词根词典...");

    let roots = sqlx::query!("SELECT cn_name FROM standard_word_roots")
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    // 获取写锁
    let mut jieba_write = JIEBA.write().await;

    // 修复第二个报错：使用 &roots 引用，避免所有权转移
    for r in &roots {
        jieba_write.add_word(&r.cn_name, Some(99999), None);
    }

    // 现在可以安全使用 roots.len()，因为 roots 没有被销毁
    tracing::info!("自定义词典加载完成，共计 {} 个词条", roots.len());
}

#[tokio::main]
async fn main() {
    // 1. 初始化日志系统
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 2. 加载 .env 环境变量
    dotenv().ok();
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env file");

    // 3. 初始化数据库连接池
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create database connection pool");

    init_custom_dictionary(&pool).await;


    // 1. 获取当前程序运行的目录（绝对路径）
    let current_dir = env::current_dir().expect("Failed to get current dir");
    // 2. 拼接出 model 文件夹的绝对路径
    let cache_path = current_dir.join("model").join("fastembed_cache");

    tracing::info!("Loading embedding model from: {:?}", cache_path);

    // 初始化 Qdrant 客户端 (默认地址)
    let qdrant = Qdrant::from_url("http://localhost:6334").build().unwrap();
    // 初始化 Embedding 模型 (ParaphraseMultilingual 适合中文)
    let model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::ParaphraseMLMiniLML12V2)
            .with_cache_dir(PathBuf::from(cache_path)) // 指定项目根目录下的 model_cache
            .with_show_download_progress(false),
    )
    .expect("离线加载失败！请检查 model/fastembed_cache 目录结构是否正确");

    // 执行预热
    init_qdrant_collection(&qdrant).await;

    let shared_state = Arc::new(AppState {
        db: pool,
        qdrant,
        embed_model: Mutex::new(model),
    });

    // 4. 配置跨域 (CORS) - 开发阶段允许所有，生产环境需收紧
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 5. 构建路由
    // 1. 认证路由 (公开)
    let auth_routes = Router::new()
        .route("/signup", post(handlers::auth_handler::signup))
        .route("/login", post(handlers::auth_handler::login));

    // 2. 用户查询路由 (公开)
    let public_routes = Router::new()
        .route("/search", get(handlers::field_handler::search_field))
        .route(
            "/similar-roots",
            get(handlers::mapping_handler::search_similar_roots),
        );

    // 3. 管理员路由 (受保护)
    let admin_routes = Router::new()
        .route(
            "/roots",
            post(handlers::word_root_handler::create_root)
                .get(handlers::word_root_handler::list_roots),
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
            "/fields/:id",
            get(handlers::field_handler::get_field_details)
                .put(handlers::field_handler::update_field)
                .delete(handlers::field_handler::delete_field),
        )
        // 新增用户管理路由
        .route(
            "/users",
            post(handlers::auth_handler::create_user_admin).get(handlers::auth_handler::list_users),
        )
        .route(
            "/users/:id",
            put(handlers::auth_handler::update_user_role)
                .delete(handlers::auth_handler::delete_user),
        )
        // 修复：建议接口属于管理员生产工具，移入 admin
        .route("/suggest", get(handlers::mapping_handler::suggest_mapping))
        .layer(axum::middleware::from_fn_with_state(
            shared_state.clone(),
            middleware::auth::guard,
        ));

    let app = Router::new()
        .nest("/api/auth", auth_routes)
        .nest("/api/public", public_routes)
        .nest("/api/admin", admin_routes)
        .with_state(shared_state)
        .layer(cors);
    // 6. 启动服务
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("🚀 Server started at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
