use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct WordRoot {
    pub id: i32,
    pub cn_name: String,
    pub en_abbr: String,
    pub en_full_name: Option<String>,
    pub associated_terms: Option<String>, // 对应 SQL 的 TEXT
    pub remark: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct CreateWordRoot {
    pub cn_name: String,
    pub en_abbr: String,
    pub en_full_name: Option<String>,
    pub associated_terms: Option<String>, // 用户输入如："钱,费用,价格"
    pub remark: Option<String>,
}