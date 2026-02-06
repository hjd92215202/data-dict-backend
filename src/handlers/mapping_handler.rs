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
    if query.q.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "查询内容不能为空").into_response();
    }

    // 调用 Service 层逻辑，Service 内部已优化为：优先匹配 cn_name，其次正则匹配同义词
    let (suggested_en, missing_words, matched_ids) =
        mapping_service::suggest_field_name(&state.db, &query.q).await;

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
    if query.q.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "查询内容不能为空").into_response();
    }

    let mut model = state.embed_model.lock().await;

    // 将查询文本转为向量
    match model.embed(vec![&query.q], None) {
        Ok(query_vector) => {
            // 在 Qdrant 的 word_roots 集合中检索最相似的 5 个词根
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

                            // 直接提取字符串，qdrant Value 的 as_str() 返回 Option<&str>
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

                    (StatusCode::OK, Json(suggestions)).into_response()
                }
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("向量库检索失败: {}", e)).into_response(),
            }
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("向量计算失败: {}", e)).into_response(),
    }
}