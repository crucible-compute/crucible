use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "crucible.dev",
    version = "v1alpha1",
    kind = "CruciblePlatform",
    namespaced,
    status = "CruciblePlatformStatus",
    shortname = "cp"
)]
pub struct CruciblePlatformSpec {
    pub celeborn: CelebornConfig,
    pub volcano: VolcanoConfig,
    pub history_server: HistoryServerConfig,
    pub object_store: ObjectStoreConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CelebornConfig {
    pub workers: u32,
    pub worker_storage: String,
    pub image: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VolcanoConfig {
    pub tenants: Vec<TenantConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TenantConfig {
    pub name: String,
    pub queue_weight: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryServerConfig {
    pub enabled: bool,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStoreConfig {
    pub bucket: String,
    pub endpoint: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CruciblePlatformStatus {
    pub phase: Option<String>,
    pub conditions: Vec<PlatformCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlatformCondition {
    pub r#type: String,
    pub status: String,
    pub message: Option<String>,
    pub last_transition_time: Option<String>,
}
