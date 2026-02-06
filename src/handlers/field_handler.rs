use axum::{extract::{State, Path, Query}, Json, http::StatusCode, response::IntoResponse};
use std::sync::Arc;
use crate::AppState;
use crate::models::field::{CreateFieldRequest, StandardField};
use crate::models::word_root::WordRoot;
use crate::handlers::mapping_handler::SuggestQuery; 
use qdrant_client::qdrant::SearchPointsBuilder;
use qdrant_client::qdrant::point_id::PointIdOptions;

/// 1. 创建标准字段
pub async fn create_field(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateFieldRequest>,
) -> impl IntoResponse {
    let result = sqlx::query_as!(
        StandardField,
        r#"
        INSERT INTO standard_fields (field_cn_name, field_en_name, composition_ids, data_type, associated_terms)
        VALUES ($1, $2, $3::INT[], $4, $5)
        RETURNING id, field_cn_name, field_en_name, composition_ids as "composition_ids!", 
                  data_type, associated_terms, is_standard as "is_standard!", created_at
        "#,
        payload.field_cn_name, payload.field_en_name, &payload.composition_ids, 
        payload.data_type, payload.associated_terms
    )
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(field) => (StatusCode::CREATED, Json(field)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("数据库错误: {}", e)).into_response(),
    }
}

/// 2. 获取所有标准字段列表
pub async fn list_fields(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let result = sqlx::query_as!(
        StandardField,
        r#"
        SELECT id, field_cn_name, field_en_name, composition_ids as "composition_ids!", 
               data_type, associated_terms, is_standard as "is_standard!", created_at
        FROM standard_fields ORDER BY created_at DESC
        "#
    ).fetch_all(&state.db).await;

    match result {
        Ok(fields) => (StatusCode::OK, Json(fields)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 3. 获取字段详情 (解析引用的词根链)
pub async fn get_field_details(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 1. 获取该字段记录
    let field_row = sqlx::query!(
        r#"SELECT composition_ids FROM standard_fields WHERE id = $1"#,
        id
    )
    .fetch_optional(&state.db)
    .await;

    match field_row {
        Ok(Some(row)) => {
            let ids = row.composition_ids.unwrap_or_default();
            if ids.is_empty() {
                return (StatusCode::OK, Json(Vec::<WordRoot>::new())).into_response();
            }

            // 2. 修正后的查询：保持 ID 顺序
            let roots = sqlx::query_as!(
                WordRoot,
                r#"
                SELECT 
                    r.id, r.cn_name, r.en_abbr, r.en_full_name, 
                    r.associated_terms, r.remark, r.created_at
                FROM UNNEST($1::INT[]) WITH ORDINALITY AS x(id, ord)
                JOIN standard_word_roots r ON r.id = x.id
                ORDER BY x.ord
                "#,
                &ids
            )
            .fetch_all(&state.db)
            .await;

            match roots {
                Ok(r) => (StatusCode::OK, Json(r)).into_response(),
                Err(e) => {
                    eprintln!("SQLx Error: {:?}", e); // 打印详细错误到终端
                    (StatusCode::INTERNAL_SERVER_ERROR, "解析词根数据失败").into_response()
                }
            }
        },
        Ok(None) => (StatusCode::NOT_FOUND, "未找到该字段").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}


/// 4. 更新标准字段
pub async fn update_field(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<CreateFieldRequest>,
) -> impl IntoResponse {
    let res = sqlx::query!(
        r#"UPDATE standard_fields SET field_cn_name=$1, field_en_name=$2, composition_ids=$3::INT[], 
           data_type=$4, associated_terms=$5 WHERE id=$6"#,
        payload.field_cn_name, payload.field_en_name, &payload.composition_ids, 
        payload.data_type, payload.associated_terms, id
    ).execute(&state.db).await;

    match res {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 5. 删除标准字段
pub async fn delete_field(State(state): State<Arc<AppState>>, Path(id): Path<i32>) -> impl IntoResponse {
    match sqlx::query!("DELETE FROM standard_fields WHERE id = $1", id).execute(&state.db).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 6. 用户端搜索接口 (支持同义词模糊匹配)
pub async fn search_field(
    State(state): State<Arc<AppState>>, 
    Query(query): Query<SuggestQuery>
) -> impl IntoResponse {
    // 1. SQL 模糊匹配 (标准名 + 同义词)
    let q = format!("%{}%", query.q);
    let sql_results = sqlx::query_as!(
        StandardField,
        r#"SELECT id, field_cn_name, field_en_name, composition_ids as "composition_ids!", 
                  data_type, associated_terms, is_standard as "is_standard!", created_at
           FROM standard_fields 
           WHERE field_cn_name ILIKE $1 OR associated_terms ILIKE $1 
           LIMIT 10"#,
        q
    ).fetch_all(&state.db).await.unwrap_or_default();

    if !sql_results.is_empty() {
        return Json(sql_results).into_response();
    }

    // 2. 向量相似度搜索 (仅在 standard_fields 集合中搜)
    let mut model = state.embed_model.lock().await;
    if let Ok(vector) = model.embed(vec![&query.q], None) {
        let search_res = state.qdrant.search_points(
            SearchPointsBuilder::new("standard_fields", vector[0].clone(), 5).with_payload(true)
        ).await;

       if let Ok(res) = search_res {
    let fields: Vec<serde_json::Value> = res.result.into_iter().map(|p| {
        let pay = p.payload;
        
        // --- 核心修复：手动解析 PointId ---
        let id_json = match p.id {
            Some(pid) => match pid.point_id_options {
                Some(PointIdOptions::Num(n)) => serde_json::json!(n),
                Some(PointIdOptions::Uuid(u)) => serde_json::json!(u),
                None => serde_json::json!(null),
            },
            None => serde_json::json!(null),
        };

        serde_json::json!({
            "id": id_json, // 使用转换后的 JSON 值
            "field_cn_name": pay.get("cn_name").and_then(|v| v.as_str()),
            "field_en_name": pay.get("en_name").and_then(|v| v.as_str()),
            "score": p.score
        })
    }).collect();
    return (StatusCode::OK, Json(fields)).into_response();
}
    }

    Json(Vec::<StandardField>::new()).into_response()
}