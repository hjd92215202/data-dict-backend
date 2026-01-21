use axum::{extract::{State, Path}, Json, http::StatusCode, response::IntoResponse};
use std::sync::Arc;
use crate::AppState;
use crate::models::field::{CreateFieldRequest, StandardField};
use crate::models::word_root::WordRoot;

/// 1. 创建标准字段 (将智能建议的结果正式入库)
pub async fn create_field(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateFieldRequest>,
) -> impl IntoResponse {
    // 使用 query_as! 宏，并显式处理 PostgreSQL 的 INT[] 数组类型映射
    let result = sqlx::query_as!(
        StandardField,
        r#"
        INSERT INTO standard_fields (field_cn_name, field_en_name, composition_ids, data_type)
        VALUES ($1, $2, $3::INT[], $4)
        RETURNING 
            id, 
            field_cn_name, 
            field_en_name, 
            composition_ids as "composition_ids!", 
            data_type, 
            is_standard as "is_standard!", 
            created_at
        "#,
        payload.field_cn_name,
        payload.field_en_name,
        &payload.composition_ids,
        payload.data_type
    )
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(field) => (StatusCode::CREATED, Json(field)).into_response(),
        Err(e) => {
            tracing::error!("Failed to create field: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("数据库错误: {}", e)).into_response()
        }
    }
}

/// 2. 获取所有标准字段列表
pub async fn list_fields(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let result = sqlx::query_as!(
        StandardField,
        r#"
        SELECT 
            id, 
            field_cn_name, 
            field_en_name, 
            composition_ids as "composition_ids!", 
            data_type, 
            is_standard as "is_standard!", 
            created_at
        FROM standard_fields
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(fields) => (StatusCode::OK, Json(fields)).into_response(),
        Err(e) => {
            tracing::error!("Failed to list fields: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

/// 3. 获取字段详情 (根据 composition_ids 解析出具体的词根信息)
/// 用于前端展示：点击某个字段，查看它是由哪几个词根组成的
pub async fn get_field_details(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 第一步：查询该字段记录
    let field_res = sqlx::query!(
        r#"SELECT composition_ids FROM standard_fields WHERE id = $1"#,
        id
    )
    .fetch_optional(&state.db)
    .await;

    match field_res {
        Ok(Some(f)) => {
            // 第二步：根据 composition_ids 数组查询所有关联词根的详细信息
            // 使用 ANY($1) 语法匹配数组中的所有 ID
            let roots_res = sqlx::query_as!(
                WordRoot,
                r#"
                SELECT 
                    id, cn_name, en_abbr, en_full_name, 
                    associated_terms, remark, created_at
                FROM standard_word_roots
                WHERE id = ANY($1::INT[])
                "#,
                &f.composition_ids.unwrap_or_default()
            )
            .fetch_all(&state.db)
            .await;

            match roots_res {
                Ok(roots) => (StatusCode::OK, Json(roots)).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("解析词根失败: {}", e)).into_response(),
            }
        },
        Ok(None) => (StatusCode::NOT_FOUND, "未找到该字段").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn update_field(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<CreateFieldRequest>,
) -> impl IntoResponse {
    let result = sqlx::query!(
        r#"
        UPDATE standard_fields 
        SET field_cn_name = $1, field_en_name = $2, composition_ids = $3::INT[], data_type = $4
        WHERE id = $5
        "#,
        payload.field_cn_name, payload.field_en_name, &payload.composition_ids, payload.data_type, id
    )
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// 删除标准字段
pub async fn delete_field(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let result = sqlx::query!("DELETE FROM standard_fields WHERE id = $1", id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}