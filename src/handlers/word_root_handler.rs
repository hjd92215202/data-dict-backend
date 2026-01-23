use crate::models::word_root::{CreateWordRoot, WordRoot};
use crate::{AppState, JIEBA};
use axum::{extract::Path, extract::State, http::StatusCode, response::IntoResponse, Json};
use std::sync::Arc;
use qdrant_client::qdrant::{PointStruct, UpsertPointsBuilder, Value};
use std::collections::HashMap;

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
        payload.cn_name, payload.en_abbr, payload.en_full_name, payload.associated_terms, payload.remark
    )
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(root) => {
            // 实时更新分词词典
            let mut jieba_write = JIEBA.write().await;
            jieba_write.add_word(&root.cn_name, Some(99999), None);

            // 计算向量并推送到 Qdrant
            let text_to_embed = format!("{}: {}", root.cn_name, root.associated_terms.as_deref().unwrap_or(""));
            let mut model = state.embed_model.lock().await;
            if let Ok(embeddings) = model.embed(vec![text_to_embed], None) {
                
                // 修复：显式构建 Payload HashMap 并指明类型
                let mut payload_map: HashMap<String, Value> = HashMap::new();
                payload_map.insert("cn_name".to_string(), root.cn_name.clone().into());
                payload_map.insert("en_abbr".to_string(), root.en_abbr.clone().into());

                let point = PointStruct::new(
                    root.id as u64, 
                    embeddings[0].clone(),
                    payload_map // 直接传入已确定类型的 HashMap
                );
                
                let _ = state.qdrant.upsert_points(UpsertPointsBuilder::new("word_roots", vec![point])).await;
            }

            (StatusCode::CREATED, Json(root)).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn list_roots(State(state): State<Arc<AppState>>) -> impl IntoResponse {
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("DB Error: {}", e),
        )
            .into_response(),
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
        payload.cn_name,
        payload.en_abbr,
        payload.en_full_name,
        payload.associated_terms,
        payload.remark,
        id
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
