use axum::{extract::State, Json, http::StatusCode, response::IntoResponse,extract::Path};
use std::sync::Arc;
use crate::AppState; 
use crate::models::word_root::{CreateWordRoot, WordRoot};

pub async fn create_root(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateWordRoot>,
) -> impl IntoResponse {
    let result = sqlx::query_as!(
        WordRoot,
        r#"
        INSERT INTO standard_word_roots (cn_name, en_abbr, en_full_name, associated_terms, remark)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, cn_name, en_abbr, en_full_name, associated_terms, remark, created_at
        "#,
        payload.cn_name,
        payload.en_abbr,
        payload.en_full_name,
        payload.associated_terms,
        payload.remark
    )
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(root) => (StatusCode::CREATED, Json(root)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    }
}

pub async fn list_roots(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let result = sqlx::query_as!(
        WordRoot,
        r#"
        SELECT id, cn_name, en_abbr, en_full_name, associated_terms, remark, created_at 
        FROM standard_word_roots 
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(roots) => (StatusCode::OK, Json(roots)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    }
}

// 更新词根
pub async fn update_root(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<CreateWordRoot>,
) -> impl IntoResponse {
    let result = sqlx::query!(
        r#"
        UPDATE standard_word_roots 
        SET cn_name = $1, en_abbr = $2, en_full_name = $3, associated_terms = $4, remark = $5
        WHERE id = $6
        "#,
        payload.cn_name, payload.en_abbr, payload.en_full_name, payload.associated_terms, payload.remark, id
    )
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// 删除词根
pub async fn delete_root(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let result = sqlx::query!("DELETE FROM standard_word_roots WHERE id = $1", id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}