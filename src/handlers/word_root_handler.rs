use crate::models::word_root::{CreateWordRoot, WordRoot};
use crate::{AppState, JIEBA};
use axum::{
    extract::Path, extract::Query, extract::State, http::StatusCode, response::IntoResponse, Json,
};
use qdrant_client::qdrant::{DeletePointsBuilder, Filter, PointStruct, UpsertPointsBuilder, Value};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub struct BatchCreateWordRoot {
    pub items: Vec<CreateWordRoot>,
}

// 批量导入的结果反馈结构
#[derive(Serialize)]
pub struct ImportResult {
    pub success_count: usize,
    pub failure_count: usize,
    pub errors: Vec<String>,
}

// 分页与搜索参数结构
#[derive(serde::Deserialize)]
pub struct PaginationQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub q: Option<String>,
}

// 分页响应结构
#[derive(serde::Serialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
}

/// 辅助函数：规范化同义词字符串（将各种分隔符统一为空格，压缩多余空格）
fn normalize_terms(input: Option<String>) -> Option<String> {
    input.map(|s| {
        s.replace(',', " ")        // 英文逗号
         .replace('，', " ")       // 中文逗号
         .split_whitespace()       // 自动处理多空格
         .collect::<Vec<_>>()
         .join(" ")                // 单空格连接
    })
}

/// 1. 创建单个词根
pub async fn create_root(
    State(state): State<Arc<AppState>>,
    Json(mut payload): Json<CreateWordRoot>,
) -> impl IntoResponse {
    // 规范化输入
    payload.associated_terms = normalize_terms(payload.associated_terms);

    tracing::info!(">>> 开始创建词根: cn_name={}, en_abbr={}", payload.cn_name, payload.en_abbr);

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
        Ok(root) => {
            // A. 更新分词
            let mut jieba_write = JIEBA.write().await;
            jieba_write.add_word(&root.cn_name, Some(99999), None);

            if let Some(terms) = &root.associated_terms {
            for term in terms.split_whitespace() {
                jieba_write.add_word(term, Some(99999), None);
            }
        }

            // B. 同步向量库
            let text_to_embed = format!(
                "{} {} {}",
                root.cn_name,
                root.en_full_name.as_deref().unwrap_or(""),
                root.associated_terms.as_deref().unwrap_or("")
            );

            let embeddings_res = {
                let mut model = state.embed_model.lock();
                model.embed(vec![text_to_embed], None)
            };

            if let Ok(embeddings) = embeddings_res {
                let mut payload_map: HashMap<String, Value> = HashMap::new();
                payload_map.insert("cn_name".to_string(), root.cn_name.clone().into());
                payload_map.insert("en_abbr".to_string(), root.en_abbr.clone().into());

                let point = PointStruct::new(root.id as u64, embeddings[0].clone(), payload_map);
                let _ = state.qdrant.upsert_points(UpsertPointsBuilder::new("word_roots", vec![point])).await;
            }

            tracing::info!("<<< 词根创建成功: ID={}", root.id);
            (StatusCode::CREATED, Json(root)).into_response()
        }
        Err(e) => {
            tracing::error!("!!! 词根创建失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("创建失败: {}", e)).into_response()
        }
    }
}

/// 2. 批量导入词根 (高性能版)
pub async fn batch_create_roots(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BatchCreateWordRoot>,
) -> impl IntoResponse {
    let total_items = payload.items.len();
    let mut success_count = 0;
    let mut errors = Vec::new();
    let mut points_to_upsert = Vec::new();

    tracing::info!(">>> 开始高性能批量导入: 总数={}", total_items);

    let mut processed_items = Vec::new();
    let mut texts_to_embed = Vec::new();

    // 数据清洗与文本准备
    for item in payload.items {
        let norm_terms = normalize_terms(item.associated_terms.clone());
        let embed_text = format!("{} {} {}", 
            item.cn_name, 
            item.en_full_name.as_deref().unwrap_or(""), 
            norm_terms.as_deref().unwrap_or("")
        );
        texts_to_embed.push(embed_text);
        processed_items.push((item, norm_terms));
    }

    // 批量向量化
    let all_embeddings = {
        let mut model = state.embed_model.lock();
        match model.embed(texts_to_embed, None) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("!!! 批量向量化失败: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "AI模型计算失败").into_response();
            }
        }
    };

    // 循环写入 DB
    for (index, (item, norm_terms)) in processed_items.into_iter().enumerate() {
        let res = sqlx::query_as!(
            WordRoot,
            r#"
            INSERT INTO standard_word_roots (cn_name, en_abbr, en_full_name, associated_terms, remark)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, cn_name, en_abbr, en_full_name, associated_terms, remark, created_at
            "#,
            item.cn_name, item.en_abbr, item.en_full_name, norm_terms, item.remark
        )
        .fetch_one(&state.db)
        .await;

        match res {
            Ok(root) => {
                success_count += 1;
                JIEBA.write().await.add_word(&root.cn_name, Some(99999), None);

                let mut payload_map: HashMap<String, Value> = HashMap::new();
                payload_map.insert("cn_name".to_string(), root.cn_name.clone().into());
                payload_map.insert("en_abbr".to_string(), root.en_abbr.clone().into());
                points_to_upsert.push(PointStruct::new(root.id as u64, all_embeddings[index].clone(), payload_map));
            },
            Err(e) => {
                errors.push(format!("行 {}: [{}] 失败: {}", index + 1, item.cn_name, e));
            }
        }
    }

    // 批量更新向量库
    if !points_to_upsert.is_empty() {
        let _ = state.qdrant.upsert_points(UpsertPointsBuilder::new("word_roots", points_to_upsert)).await;
    }

    tracing::info!("<<< 批量处理完成: 成功={}", success_count);
    (StatusCode::OK, Json(ImportResult { success_count, failure_count: errors.len(), errors })).into_response()
}

/// 3. 获取分页词根列表 (增加同义词搜索支持)
pub async fn list_roots(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PaginationQuery>,
) -> impl IntoResponse {
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(20);
    let offset = (page - 1) * page_size;
    let search_q = query.q.as_deref().unwrap_or("");

    // A. 统计总数 (搜索 cn_name, en_abbr, associated_terms)
    let total = if search_q.is_empty() {
        sqlx::query_scalar!("SELECT count(*) FROM standard_word_roots")
            .fetch_one(&state.db).await.unwrap_or(Some(0)).unwrap_or(0)
    } else {
        let pattern = format!("%{}%", search_q);
        sqlx::query_scalar!(
            "SELECT count(*) FROM standard_word_roots 
             WHERE cn_name ILIKE $1 OR en_abbr ILIKE $1 OR associated_terms ILIKE $1", 
            pattern
        ).fetch_one(&state.db).await.unwrap_or(Some(0)).unwrap_or(0)
    };

    // B. 获取分页数据
    let items_res = if search_q.is_empty() {
        sqlx::query_as!(
            WordRoot, 
            "SELECT * FROM standard_word_roots ORDER BY created_at DESC LIMIT $1 OFFSET $2", 
            page_size, offset
        ).fetch_all(&state.db).await
    } else {
        let pattern = format!("%{}%", search_q);
        sqlx::query_as!(
            WordRoot, 
            "SELECT * FROM standard_word_roots 
             WHERE cn_name ILIKE $1 OR en_abbr ILIKE $1 OR associated_terms ILIKE $1 
             ORDER BY created_at DESC LIMIT $2 OFFSET $3", 
            pattern, page_size, offset
        ).fetch_all(&state.db).await
    };

    match items_res {
        Ok(items) => (StatusCode::OK, Json(PaginatedResponse { items, total })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("查询异常: {}", e)).into_response(),
    }
}

/// 4. 更新词根
pub async fn update_root(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(mut payload): Json<CreateWordRoot>,
) -> impl IntoResponse {
    payload.associated_terms = normalize_terms(payload.associated_terms);
    tracing::info!(">>> 更新词根 ID: {}", id);

    let result = sqlx::query_as!(
        WordRoot,
        r#"
        UPDATE standard_word_roots 
        SET cn_name = $1, en_abbr = $2, en_full_name = $3, associated_terms = $4, remark = $5
        WHERE id = $6
        RETURNING id, cn_name, en_abbr, en_full_name, associated_terms, remark, created_at
        "#,
        payload.cn_name,
        payload.en_abbr,
        payload.en_full_name,
        payload.associated_terms,
        payload.remark,
        id
    )
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(root) => {
            let text = format!("{} {} {}", 
                root.cn_name, 
                root.en_full_name.as_deref().unwrap_or(""), 
                root.associated_terms.as_deref().unwrap_or("")
            );
            
            let embeddings_res = {
                let mut model = state.embed_model.lock();
                model.embed(vec![text], None)
            };

            if let Ok(embeddings) = embeddings_res {
                let mut payload_map: HashMap<String, Value> = HashMap::new();
                payload_map.insert("cn_name".to_string(), root.cn_name.clone().into());
                payload_map.insert("en_abbr".to_string(), root.en_abbr.clone().into());
                let point = PointStruct::new(root.id as u64, embeddings[0].clone(), payload_map);
                let _ = state.qdrant.upsert_points(UpsertPointsBuilder::new("word_roots", vec![point])).await;
            }
            StatusCode::OK.into_response()
        }
        Err(e) => {
            tracing::error!("!!! 更新失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "更新失败").into_response()
        }
    }
}

/// 5. 删除词根
pub async fn delete_root(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let result = sqlx::query!("DELETE FROM standard_word_roots WHERE id = $1", id).execute(&state.db).await;

    match result {
        Ok(res) => {
            if res.rows_affected() > 0 {
                let _ = state.qdrant.delete_points(DeletePointsBuilder::new("word_roots").points(vec![id as u64])).await;
                StatusCode::NO_CONTENT.into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("删除异常: {}", e)).into_response(),
    }
}

/// 6. 一键清空所有词根
pub async fn clear_all_roots(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::warn!("⚠️ 执行全量清空词根库");
    let db_res = sqlx::query!("TRUNCATE TABLE standard_word_roots RESTART IDENTITY").execute(&state.db).await;

    match db_res {
        Ok(_) => {
            let _ = state.qdrant.delete_points(DeletePointsBuilder::new("word_roots").points(Filter::default())).await;
            (StatusCode::OK, "所有词根数据已成功清空").into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("清空异常: {}", e)).into_response(),
    }
}