use sqlx::PgPool;
use crate::models::word_root::WordRoot;
use crate::JIEBA;

pub async fn suggest_field_name(pool: &PgPool, cn_input: &str) -> (String, Vec<String>, Vec<i32>) {
    let words = JIEBA.cut(cn_input, false);
    let mut en_parts = Vec::new();
    let mut missing_words = Vec::new();
    let mut matched_ids = Vec::new(); // 新增：存储匹配到的 ID

    for word in words {
        let root = sqlx::query_as!(
            WordRoot,
            r#"SELECT * FROM standard_word_roots WHERE cn_name = $1 OR associated_terms ILIKE $2 LIMIT 1"#,
            word,
            format!("%{}%", word)
        )
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

        match root {
            Some(r) => {
                en_parts.push(r.en_abbr);
                matched_ids.push(r.id); // 记录 ID
            },
            None => {
                if !word.trim().is_empty() {
                    missing_words.push(word.to_string());
                    en_parts.push(format!("[{}]", word));
                }
            }
        }
    }
    (en_parts.join("_"), missing_words, matched_ids)
}