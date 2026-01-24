use tracing::info;
use sqlx::SqlitePool;


pub struct NodeManager {
    pool: SqlitePool,
}

impl NodeManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn add_node(
        &self,
        id: i64,
    ) {
        info!("Registering node ID: {}", id);
        
        let _ = sqlx::query("UPDATE nodes SET status = 'new' WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await;
    }
}
