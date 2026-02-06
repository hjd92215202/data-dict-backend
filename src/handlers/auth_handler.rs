use axum::{extract::State, Json, http::StatusCode, response::IntoResponse};
use std::sync::Arc;
use crate::{AppState, models::user::{User, Claims}};
use argon2::{Argon2, PasswordHash, PasswordVerifier, password_hash::{SaltString, PasswordHasher}};
use jsonwebtoken::{encode, Header, EncodingKey};
use serde::{Deserialize, Serialize};
use chrono::Utc;
use rand::rngs::OsRng;
use axum::extract::Path;

#[derive(Deserialize)]
pub struct AuthPayload {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub role: String,
}

#[derive(Deserialize)]
pub struct AdminCreateUserPayload {
    pub username: String,
    pub password: String,
    pub role: String,
}

/// 用户登录
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AuthPayload>,
) -> impl IntoResponse {
    tracing::info!(">>> 登录尝试: username={}", payload.username);

    // 显式映射字段，确保 password_hash 和 role 非空
    let user = sqlx::query_as!(
        User, 
        r#"SELECT id, username, password_hash as "password_hash!", role as "role!", created_at FROM users WHERE username = $1"#, 
        payload.username
    )
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    if let Some(user) = user {
        if let Ok(parsed_hash) = PasswordHash::new(&user.password_hash) {
            if Argon2::default().verify_password(payload.password.as_bytes(), &parsed_hash).is_ok() {
                let claims = Claims {
                    sub: user.id,
                    exp: (Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
                    role: user.role.clone(),
                };
                
                let token = encode(
                    &Header::default(), 
                    &claims, 
                    &EncodingKey::from_secret("secret_key".as_ref())
                ).unwrap();

                tracing::info!("<<< 登录成功: username={}, role={}, id={}", user.username, user.role, user.id);
                return (StatusCode::OK, Json(AuthResponse { token, role: user.role })).into_response();
            } else {
                tracing::warn!("--- 登录失败: 用户[{}]密码校验未通过", payload.username);
            }
        }
    } else {
        tracing::warn!("--- 登录失败: 用户名[{}]不存在", payload.username);
    }
    
    (StatusCode::UNAUTHORIZED, "用户名或密码错误").into_response()
}

/// 用户注册 (自主注册)
pub async fn signup(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AuthPayload>,
) -> impl IntoResponse {
    tracing::info!(">>> 收到自主注册请求: username={}", payload.username);

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(payload.password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .unwrap_or_default();

    let res = sqlx::query!(
        "INSERT INTO users (username, password_hash, role) VALUES ($1, $2, $3)",
        payload.username, password_hash, "user"
    )
    .execute(&state.db)
    .await;

    match res {
        Ok(_) => {
            tracing::info!("<<< 用户注册成功: username={}", payload.username);
            StatusCode::CREATED.into_response()
        },
        Err(e) => {
            tracing::error!("!!! 用户注册失败: username={}, Error: {}", payload.username, e);
            (StatusCode::BAD_REQUEST, "用户已存在或数据库异常").into_response()
        },
    }
}

/// 管理员直接创建用户
pub async fn create_user_admin(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AdminCreateUserPayload>,
) -> impl IntoResponse {
    tracing::info!(">>> 管理员手动创建用户: username={}, role={}", payload.username, payload.role);

    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    let password_hash = Argon2::default()
        .hash_password(payload.password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .unwrap_or_default();

    let res = sqlx::query!(
        "INSERT INTO users (username, password_hash, role) VALUES ($1, $2, $3)",
        payload.username, 
        password_hash, 
        payload.role
    )
    .execute(&state.db)
    .await;

    match res {
        Ok(_) => {
            tracing::info!("<<< 管理员创建用户成功: username={}", payload.username);
            StatusCode::CREATED.into_response()
        },
        Err(e) => {
            tracing::error!("!!! 管理员创建用户失败: {}, Error: {}", payload.username, e);
            (StatusCode::BAD_REQUEST, "用户名已存在或参数错误").into_response()
        },
    }
}

/// 1. 获取所有用户列表
pub async fn list_users(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    tracing::debug!(">>> 正在获取全量用户列表进行权限管理");

    let result = sqlx::query_as!(
        User,
        r#"SELECT id, username, password_hash as "password_hash!", role as "role!", created_at FROM users ORDER BY id ASC"#
    )
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(users) => {
            tracing::debug!("<<< 用户列表获取完毕, 数量: {}", users.len());
            (StatusCode::OK, Json(users)).into_response()
        },
        Err(e) => {
            tracing::error!("!!! 获取用户列表异常: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        },
    }
}

/// 2. 修改用户角色 (权限变更)
pub async fn update_user_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<serde_json::Value>, 
) -> impl IntoResponse {
    let role = payload["role"].as_str().unwrap_or("user");
    tracing::info!(">>> 正在变更用户角色: ID={}, 新角色={}", id, role);
    
    let result = sqlx::query!(
        "UPDATE users SET role = $1 WHERE id = $2",
        role, id
    )
    .execute(&state.db)
    .await;

    match result {
        Ok(res) => {
            if res.rows_affected() > 0 {
                tracing::info!("<<< 角色更新成功: ID={}", id);
                StatusCode::OK.into_response()
            } else {
                tracing::warn!("--- 尝试更新不存在的用户角色: ID={}", id);
                StatusCode::NOT_FOUND.into_response()
            }
        },
        Err(e) => {
            tracing::error!("!!! 角色更新失败: ID={}, Error: {}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        },
    }
}

/// 3. 删除用户
pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    tracing::warn!(">>> 正在删除用户账号: ID={}", id);

    let result = sqlx::query!("DELETE FROM users WHERE id = $1", id)
        .execute(&state.db)
        .await;

    match result {
        Ok(res) => {
            if res.rows_affected() > 0 {
                tracing::info!("<<< 用户账号 ID={} 已注销", id);
                StatusCode::NO_CONTENT.into_response()
            } else {
                tracing::warn!("--- 尝试删除不存在的用户账号: ID={}", id);
                StatusCode::NOT_FOUND.into_response()
            }
        },
        Err(e) => {
            tracing::error!("!!! 用户账号删除异常: ID={}, Error: {}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        },
    }
}