use axum::{extract::{State, Query}, Json, response::IntoResponse};
use std::sync::Arc;
use crate::AppState;
use crate::services::mapping_service;
use serde::{Deserialize, Serialize};

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

pub async fn suggest_mapping(
    State(state): State<Arc<AppState>>, // 注意这里改成了 AppState
    Query(query): Query<SuggestQuery>,
) -> impl IntoResponse {
    let (suggested_en, missing_words, matched_ids) = 
        mapping_service::suggest_field_name(&state.db, &query.q).await;
        
    Json(SuggestResponse {
        suggested_en,
        missing_words,
        matched_ids,
    })
}