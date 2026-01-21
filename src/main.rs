use axum::{
    routing::{get, post, put},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use sqlx::postgres::{PgPool, PgPoolOptions};
use dotenvy::dotenv;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use jieba_rs::Jieba;
use once_cell::sync::Lazy;

// 声明子模块
mod models;
mod handlers;
mod services;

// 使用 Lazy 确保 Jieba 词库只在启动时加载一次，并全局可用
pub static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

// 定义全局状态，方便在 Handler 中获取数据库连接池
pub struct AppState {
    pub db: PgPool,
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
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");

    // 3. 初始化数据库连接池
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create database connection pool");

    let shared_state = Arc::new(AppState { db: pool });

    // 4. 配置跨域 (CORS) - 开发阶段允许所有，生产环境需收紧
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // 5. 构建路由
    let app = Router::new()
        // 词根相关接口
        .route("/api/roots", post(handlers::word_root_handler::create_root)
            .get(handlers::word_root_handler::list_roots))
        .route("/api/roots/:id", put(handlers::word_root_handler::update_root)
            .delete(handlers::word_root_handler::delete_root))
        
        // 字段接口 (新增)
        .route("/api/fields", post(handlers::field_handler::create_field)
            .get(handlers::field_handler::list_fields))
        .route("/api/fields/:id", get(handlers::field_handler::get_field_details)
            .put(handlers::field_handler::update_field)
            .delete(handlers::field_handler::delete_field)) 

        // 智能映射接口 (中文转英文建议)
        .route("/api/suggest", get(handlers::mapping_handler::suggest_mapping))
        
        // 中间件：日志记录和跨域
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(shared_state);

    // 6. 启动服务
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("🚀 Server started at http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}