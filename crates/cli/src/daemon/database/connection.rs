use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let connection_string = format!("sqlite:{}", path.display());

        let options = SqliteConnectOptions::from_str(&connection_string)?.create_if_missing(true);

        let pool = SqlitePoolOptions::new().connect_with(options).await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    #[cfg(test)]
    pub async fn in_memory() -> Result<Self> {
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_database_creation() {
        let db = Database::in_memory().await.unwrap();
        assert!(!db.pool().is_closed());
    }

    #[tokio::test]
    async fn test_migrations_create_tables() {
        let db = Database::in_memory().await.unwrap();

        let result: (i32,) =
            sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='runs'")
                .fetch_one(db.pool())
                .await
                .unwrap();

        assert_eq!(result.0, 1);
    }
}
