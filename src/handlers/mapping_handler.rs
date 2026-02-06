use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use qdrant_client::qdrant::{point_id::PointIdOptions, SearchPointsBuilder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::services::mapping_service;
use crate::AppState;

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

#[derive(Serialize)]
pub struct RootSuggestion {
    pub id: String,
    pub cn_name: String,
    pub en_abbr: String,
    pub score: f32,
}

/// 1. 分词建议接口 (管理员生产标准字段的核心工具)
/// 逻辑：将中文输入利用 JIEBA 切分，并匹配标准词根库（含同义词匹配）
pub async fn suggest_mapping(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SuggestQuery>,
) -> impl IntoResponse {
    let input = query.q.trim();
    if input.is_empty() {
        tracing::warn!("--- 收到空的分词建议请求");
        return (StatusCode::BAD_REQUEST, "查询内容不能为空").into_response();
    }

    tracing::info!(">>> 正在为管理员生成分词建议: q='{}'", input);

    // 调用 Service 层逻辑
    let (suggested_en, missing_words, matched_ids) =
        mapping_service::suggest_field_name(&state.db, input).await;

    if !missing_words.is_empty() {
        tracing::warn!("--- 词汇未完全标准化: 缺失词汇={:?}", missing_words);
    }

    tracing::info!(
        "<<< 建议生成成功: en_abbr={}, matched_count={}",
        suggested_en,
        matched_ids.len()
    );

    Json(SuggestResponse {
        suggested_en,
        missing_words,
        matched_ids,
    })
    .into_response()
}

/// 2. 语义相似度搜索词根 (生产辅助)
/// 场景 A：管理员发现某个词没词根，想搜一下有没有意思相近的存量词根
/// 场景 B：普通用户搜不到标准字段时，展示“相关词根”供参考
pub async fn search_similar_roots(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SuggestQuery>,
) -> impl IntoResponse {
    let input = query.q.trim();
    if input.is_empty() {
        return (StatusCode::BAD_REQUEST, "查询内容不能为空").into_response();
    }

    tracing::info!(">>> 正在检索语义相近词根: q='{}'", input);

    let mut model = state.embed_model.lock().await;

    // 1. 将查询文本转为向量
    tracing::debug!("--- 正在计算输入文本向量: '{}'", input);
    match model.embed(vec![input], None) {
        Ok(query_vector) => {
            // 2. 在 Qdrant 的 word_roots 集合中检索最相似的 5 个词根
            let search_res = state.qdrant
                .search_points(
                    SearchPointsBuilder::new("word_roots", query_vector[0].clone(), 5)
                        .with_payload(true),
                )
                .await;

            match search_res {
                Ok(res) => {
                    let suggestions: Vec<RootSuggestion> = res
                        .result
                        .into_iter()
                        .map(|p| {
                            let pay = p.payload;

                            // 解析 ID
                            let id_str = match p.id {
                                Some(pid) => match pid.point_id_options {
                                    Some(PointIdOptions::Num(n)) => n.to_string(),
                                    Some(PointIdOptions::Uuid(u)) => u,
                                    None => "0".to_string(),
                                },
                                None => "0".to_string(),
                            };

                            let cn_name = pay.get("cn_name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.as_str()) 
                                .unwrap_or("")
                                .to_string();

                            let en_abbr = pay.get("en_abbr")
                                .and_then(|v| v.as_str())
                                .map(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();

                            RootSuggestion {
                                id: id_str,
                                cn_name,
                                en_abbr,
                                score: p.score,
                            }
                        })
                        .collect();

                    tracing::info!("<<< 语义搜索完成: 召回数量={}", suggestions.len());
                    (StatusCode::OK, Json(suggestions)).into_response()
                }
                Err(e) => {
                    tracing::error!("!!! Qdrant 检索词根异常: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("向量库检索失败: {}", e)).into_response()
                },
            }
        }
        Err(e) => {
            tracing::error!("!!! 向量模型计算异常: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("向量计算失败: {}", e)).into_response()
        },
    }
}