use bytes::Bytes;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("object not found: {0}")]
    NotFound(String),

    #[error("store error: {0}")]
    Internal(String),
}

/// Trait abstracting object store operations.
#[allow(async_fn_in_trait)]
pub trait ObjectStore: Send + Sync {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StoreError>;
    async fn get(&self, key: &str) -> Result<Bytes, StoreError>;
    async fn get_range(&self, key: &str, offset: u64, length: u64) -> Result<Bytes, StoreError>;
    async fn delete(&self, key: &str) -> Result<(), StoreError>;
}
