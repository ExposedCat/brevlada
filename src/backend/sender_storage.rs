use super::Storage;
use crate::models::Message;
use anyhow::Result;
use rusqlite::params;

impl Storage {
    pub fn sender_headers(&self, account: &str, folder: &str) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT json_set(data, '$.body_html', '', '$.attachments', json('[]'),
             '$.inline_media', json('[]'), '$.parcels', json('[]'),
             '$.body_loaded', json('false'), '$.inline_media_loaded', json('false'))
             FROM rust_messages WHERE account_id=?1 AND folder=?2
             ORDER BY json_extract(data, '$.timestamp') DESC, uid DESC",
        )?;
        let rows = statement.query_map(params![account, folder], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
}
