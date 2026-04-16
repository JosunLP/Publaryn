use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{
    config::{Credentials, Region},
    primitives::ByteStream,
    Client,
};
use bytes::Bytes;

use publaryn_core::{Error, Result};

use crate::config::StorageConfig;

#[derive(Debug, Clone)]
pub struct PutArtifactObject {
    pub storage_key: String,
    pub content_type: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone)]
pub struct StoredArtifactObject {
    pub content_type: String,
    pub bytes: Bytes,
}

#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn put_object(&self, object: PutArtifactObject) -> Result<()>;
    async fn get_object(&self, storage_key: &str) -> Result<Option<StoredArtifactObject>>;
}

/// In-memory artifact store for testing.
#[derive(Debug, Default)]
pub struct MemoryArtifactStore {
    objects: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, StoredArtifactObject>>>,
}

impl MemoryArtifactStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ArtifactStore for MemoryArtifactStore {
    async fn put_object(&self, object: PutArtifactObject) -> Result<()> {
        self.objects.write().await.insert(
            object.storage_key,
            StoredArtifactObject {
                content_type: object.content_type,
                bytes: object.bytes,
            },
        );
        Ok(())
    }

    async fn get_object(&self, storage_key: &str) -> Result<Option<StoredArtifactObject>> {
        Ok(self.objects.read().await.get(storage_key).cloned())
    }
}

pub struct S3ArtifactStore {
    client: Client,
    bucket: String,
}

impl S3ArtifactStore {
    pub async fn new(cfg: &StorageConfig) -> Result<Self> {
        let credentials = Credentials::new(
            cfg.access_key.clone(),
            cfg.secret_key.clone(),
            None,
            None,
            "publaryn-config",
        );

        let shared_config = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(cfg.region.clone()))
            .credentials_provider(credentials)
            .load()
            .await;

        let service_config = aws_sdk_s3::config::Builder::from(&shared_config)
            .endpoint_url(&cfg.endpoint)
            .force_path_style(true)
            .build();

        Ok(Self {
            client: Client::from_conf(service_config),
            bucket: cfg.bucket.clone(),
        })
    }
}

#[async_trait]
impl ArtifactStore for S3ArtifactStore {
    async fn put_object(&self, object: PutArtifactObject) -> Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&object.storage_key)
            .content_type(object.content_type)
            .body(ByteStream::from(object.bytes))
            .send()
            .await
            .map_err(|error| Error::Internal(format!("Artifact storage upload failed: {error}")))?;

        Ok(())
    }

    async fn get_object(&self, storage_key: &str) -> Result<Option<StoredArtifactObject>> {
        let response = match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                if error
                    .as_service_error()
                    .is_some_and(|service_error| service_error.is_no_such_key())
                {
                    return Ok(None);
                }

                return Err(Error::Internal(format!(
                    "Artifact storage download failed: {error}"
                )));
            }
        };

        let content_type = response
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_owned();
        let bytes = response
            .body
            .collect()
            .await
            .map_err(|error| Error::Internal(format!("Artifact storage stream failed: {error}")))?
            .into_bytes();

        Ok(Some(StoredArtifactObject {
            content_type,
            bytes,
        }))
    }
}

// ── ArtifactStoreReader bridge ────────────────────────────────────────────────

/// Adapter that implements the workers crate's [`ArtifactStoreReader`] trait
/// by delegating to any [`ArtifactStore`] implementor.
pub struct ArtifactStoreReaderAdapter {
    inner: std::sync::Arc<dyn ArtifactStore>,
}

impl ArtifactStoreReaderAdapter {
    pub fn new(store: std::sync::Arc<dyn ArtifactStore>) -> Self {
        Self { inner: store }
    }
}

#[async_trait]
impl publaryn_workers::scanners::ArtifactStoreReader for ArtifactStoreReaderAdapter {
    async fn get_object_bytes(&self, storage_key: &str) -> Result<Option<Bytes>, String> {
        self.inner
            .get_object(storage_key)
            .await
            .map(|opt| opt.map(|o| o.bytes))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{ArtifactStore, MemoryArtifactStore, PutArtifactObject};

    #[tokio::test]
    async fn memory_store_round_trips_artifact_bytes() {
        let store = MemoryArtifactStore::new();

        store
            .put_object(PutArtifactObject {
                storage_key: "releases/demo/example".into(),
                content_type: "application/octet-stream".into(),
                bytes: Bytes::from_static(b"demo"),
            })
            .await
            .expect("memory upload should succeed");

        let stored = store
            .get_object("releases/demo/example")
            .await
            .expect("memory download should succeed")
            .expect("object should exist");

        assert_eq!(stored.content_type, "application/octet-stream");
        assert_eq!(stored.bytes, Bytes::from_static(b"demo"));
    }
}
