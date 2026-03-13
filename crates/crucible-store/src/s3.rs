use aws_sdk_s3::Client;
use bytes::Bytes;

use crate::client::{ObjectStore, StoreError};

/// S3-compatible object store implementation.
/// Works with AWS S3 and MinIO (via endpoint override).
pub struct S3Store {
    client: Client,
    bucket: String,
}

impl S3Store {
    pub fn new(client: Client, bucket: String) -> Self {
        Self { client, bucket }
    }
}

impl ObjectStore for S3Store {
    async fn put(&self, key: &str, data: Bytes) -> Result<(), StoreError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.into())
            .send()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes, StoreError> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        let body = resp
            .body
            .collect()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(body.into_bytes())
    }

    async fn get_range(&self, key: &str, offset: u64, length: u64) -> Result<Bytes, StoreError> {
        let range = format!("bytes={}-{}", offset, offset + length - 1);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .range(range)
            .send()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        let body = resp
            .body
            .collect()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;

        Ok(body.into_bytes())
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }
}
