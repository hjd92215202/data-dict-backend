use axum::{extract::{State, Path}, Json, http::StatusCode, response::IntoResponse};
use std::sync::Arc;
use crate::AppState;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct CreateTaskPayload {
    pub field_cn_name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct NotificationTask {
    pub id: i32,
    pub task_type: String,
    pub payload: serde_json::Value,
    pub is_read: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 用户提交新增申请
pub async fn submit_task(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateTaskPayload>,
) -> impl IntoResponse {
    tracing::info!(">>> 用户提交新字段申请: {}", payload.field_cn_name);
    
    let res = sqlx::query!(
        "INSERT INTO notification_tasks (task_type, payload) VALUES ($1, $2)",
        "FIELD_REQUEST",
        serde_json::json!({ "field_cn_name": payload.field_cn_name })
    )
    .execute(&state.db)
    .await;

    match res {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => {
            tracing::error!("!!! 提交申请失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "提交失败").into_response()
        }
    }
}

/// 管理员获取待办任务列表
pub async fn list_tasks(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let result = sqlx::query_as!(
        NotificationTask,
        "SELECT id, task_type, payload, is_read as \"is_read!\", created_at as \"created_at!\" 
         FROM notification_tasks WHERE is_read = false ORDER BY created_at DESC"
    )
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 管理员标记任务为已处理
pub async fn complete_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let res = sqlx::query!("UPDATE notification_tasks SET is_read = true WHERE id = $1", id)
        .execute(&state.db)
        .await;

    match res {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn count_unprocessed_tasks(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let res = sqlx::query_scalar!(
        "SELECT count(*) FROM notification_tasks WHERE is_read = false"
    )
    .fetch_one(&state.db)
    .await;

    match res {
        Ok(count) => Json(serde_json::json!({ "count": count.unwrap_or(0) })).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "查询失败").into_response(),
    }
}