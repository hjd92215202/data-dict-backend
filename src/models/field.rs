use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct StandardField {
    pub id: i32,
    pub field_cn_name: String,
    pub field_en_name: String,
    pub composition_ids: Vec<i32>, // 关联的词根ID数组
    pub data_type: Option<String>,
    pub is_standard: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct CreateFieldRequest {
    pub field_cn_name: String,
    pub field_en_name: String,
    pub composition_ids: Vec<i32>,
    pub data_type: Option<String>,
}