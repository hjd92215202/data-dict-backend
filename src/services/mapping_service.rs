use sqlx::PgPool;
use crate::models::word_root::WordRoot;

pub async fn suggest_field_name(pool: &PgPool, cn_input: &str) -> (String, Vec<String>, Vec<i32>) {
    let jieba_read = crate::JIEBA.read().await;
    let words = jieba_read.cut(cn_input, false);
    
    let mut en_parts = Vec::new();
    let mut missing_words = Vec::new();
    let mut matched_ids = Vec::new();

    for word in words {
        if word.trim().is_empty() { continue; }
        
        // 同时匹配中文名和关联词 (ILIKE 是为了兼容同义词)
        let root = sqlx::query_as!(
            WordRoot,
            r#"SELECT * FROM standard_word_roots 
               WHERE cn_name = $1 
               OR associated_terms ~* $2 
               LIMIT 1"#,
            word,
            format!(r"(^|,){}(,|$)", word) // 正则匹配独立词
        )
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

        match root {
            Some(r) => {
                en_parts.push(r.en_abbr);
                matched_ids.push(r.id);
            },
            None => {
                missing_words.push(word.to_string());
                en_parts.push(format!("[{}]", word));
            }
        }
    }
    (en_parts.join("_"), missing_words, matched_ids)
}