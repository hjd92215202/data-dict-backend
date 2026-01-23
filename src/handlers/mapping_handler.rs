use axum::{extract::{State, Query}, Json, response::IntoResponse, http::StatusCode};
use std::sync::Arc;
use crate::AppState;
use crate::services::mapping_service;
use serde::{Deserialize, Serialize};
use qdrant_client::qdrant::{SearchPointsBuilder, point_id::PointIdOptions};

#[derive(Deserialize)]
pub struct SuggestQuery {
    pub q: String,
}

#[derive(Serialize)]
pub struct SuggestResponse {
    pub suggested_en: String,
    pub missing_words: Vec<String>,
    pub matched_ids: Vec<i32>,
}

/// 1. 分词建议接口 (管理员用)
pub async fn suggest_mapping(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SuggestQuery>,
) -> impl IntoResponse {
    // 逻辑已封装在 service 中，内部需处理 JIEBA 锁
    let (suggested_en, missing_words, matched_ids) = 
        mapping_service::suggest_field_name(&state.db, &query.q).await;
        
    Json(SuggestResponse { suggested_en, missing_words, matched_ids })
}

/// 2. 语义相似度搜索接口 (普通用户搜不到时调用)
pub async fn search_similar_roots(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SuggestQuery>,
) -> impl IntoResponse {

    let mut model = state.embed_model.lock().await;

    // 将查询文本转为向量
    match model.embed(vec![&query.q], None) {
        Ok(query_vector) => {
            // 在 Qdrant 中检索最相似的 5 个词根
            let search_res = state.qdrant.search_points(
                SearchPointsBuilder::new("word_roots", query_vector[0].clone(), 5)
                    .with_payload(true)
            ).await;

            match search_res {
                Ok(res) => {
                    let suggestions: Vec<serde_json::Value> = res.result.into_iter().map(|p| {
                        let pay = p.payload;
                        
                        // 1. 手动解析 PointId 为字符串
                        let id_str = match p.id {
                            Some(pid) => match pid.point_id_options {
                                Some(PointIdOptions::Num(n)) => n.to_string(),
                                Some(PointIdOptions::Uuid(u)) => u,
                                None => "unknown".to_string(),
                            },
                            None => "none".to_string(),
                        };

                        // 2. 核心修复点：使用 .map(|s| s.as_str()) 将 &String 转为 &str
                        // 这样 unwrap_or("") 才能匹配类型
                        let cn_name = pay.get("cn_name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.as_str()) 
                            .unwrap_or("");

                        let en_abbr = pay.get("en_abbr")
                            .and_then(|v| v.as_str())
                            .map(|s| s.as_str())
                            .unwrap_or("");

                        serde_json::json!({
                            "id": id_str,
                            "cn_name": cn_name,
                            "en_abbr": en_abbr,
                            "score": p.score
                        })
                    }).collect();
                    
                    (StatusCode::OK, Json(suggestions)).into_response()
                },
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            }
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}