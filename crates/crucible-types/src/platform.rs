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
#[serde(rename_all = "camelCase")]
pub struct CruciblePlatformSpec {
    pub celeborn: CelebornConfig,
    pub volcano: VolcanoConfig,
    pub history_server: HistoryServerConfig,
    pub object_store: ObjectStoreConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CelebornConfig {
    pub workers: u32,
    pub worker_storage: StorageType,
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum StorageType {
    #[serde(rename = "emptyDir")]
    EmptyDir,
    #[serde(rename = "nvme")]
    Nvme,
    #[serde(rename = "gp3")]
    Gp3,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VolcanoConfig {
    pub tenants: Vec<TenantConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct CruciblePlatformStatus {
    pub phase: Option<PlatformPhase>,
    #[serde(default)]
    pub conditions: Vec<PlatformCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlatformPhase {
    Pending,
    Deploying,
    Ready,
    Degraded,
    Error,
}

impl std::fmt::Display for PlatformPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Deploying => write!(f, "Deploying"),
            Self::Ready => write!(f, "Ready"),
            Self::Degraded => write!(f, "Degraded"),
            Self::Error => write!(f, "Error"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ConditionType {
    CelebornReady,
    VolcanoReady,
    HistoryServerReady,
    FlinkOperatorReady,
    ApiServerReady,
}

impl std::fmt::Display for ConditionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CelebornReady => write!(f, "CelebornReady"),
            Self::VolcanoReady => write!(f, "VolcanoReady"),
            Self::HistoryServerReady => write!(f, "HistoryServerReady"),
            Self::FlinkOperatorReady => write!(f, "FlinkOperatorReady"),
            Self::ApiServerReady => write!(f, "ApiServerReady"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ConditionStatus {
    True,
    False,
    Unknown,
}

impl std::fmt::Display for ConditionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::True => write!(f, "True"),
            Self::False => write!(f, "False"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCondition {
    pub r#type: ConditionType,
    pub status: ConditionStatus,
    pub message: Option<String>,
    pub last_transition_time: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> CruciblePlatformSpec {
        CruciblePlatformSpec {
            celeborn: CelebornConfig {
                workers: 3,
                worker_storage: StorageType::EmptyDir,
                image: "apache/celeborn:0.5.2".to_string(),
            },
            volcano: VolcanoConfig {
                tenants: vec![
                    TenantConfig {
                        name: "default".to_string(),
                        queue_weight: Some(1),
                    },
                    TenantConfig {
                        name: "team-a".to_string(),
                        queue_weight: Some(2),
                    },
                ],
            },
            history_server: HistoryServerConfig {
                enabled: true,
                image: Some("crucible-spark:latest".to_string()),
            },
            object_store: ObjectStoreConfig {
                bucket: "crucible-data".to_string(),
                endpoint: Some("http://minio:9000".to_string()),
                region: Some("us-east-1".to_string()),
            },
        }
    }

    #[test]
    fn spec_serialization_round_trip() {
        let spec = sample_spec();
        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: CruciblePlatformSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.celeborn.workers, 3);
        assert_eq!(deserialized.celeborn.worker_storage, StorageType::EmptyDir);
        assert_eq!(deserialized.volcano.tenants.len(), 2);
        assert!(deserialized.history_server.enabled);
        assert_eq!(deserialized.object_store.bucket, "crucible-data");
    }

    #[test]
    fn storage_type_serializes_correctly() {
        assert_eq!(
            serde_json::to_string(&StorageType::EmptyDir).unwrap(),
            "\"emptyDir\""
        );
        assert_eq!(
            serde_json::to_string(&StorageType::Nvme).unwrap(),
            "\"nvme\""
        );
        assert_eq!(serde_json::to_string(&StorageType::Gp3).unwrap(), "\"gp3\"");
    }

    #[test]
    fn status_round_trip() {
        let status = CruciblePlatformStatus {
            phase: Some(PlatformPhase::Ready),
            conditions: vec![PlatformCondition {
                r#type: ConditionType::CelebornReady,
                status: ConditionStatus::True,
                message: Some("All workers ready".to_string()),
                last_transition_time: Some("2026-01-01T00:00:00Z".to_string()),
            }],
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: CruciblePlatformStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.phase, Some(PlatformPhase::Ready));
        assert_eq!(deserialized.conditions.len(), 1);
        assert_eq!(
            deserialized.conditions[0].r#type,
            ConditionType::CelebornReady
        );
        assert_eq!(deserialized.conditions[0].status, ConditionStatus::True);
    }

    #[test]
    fn condition_status_display() {
        assert_eq!(ConditionStatus::True.to_string(), "True");
        assert_eq!(ConditionStatus::False.to_string(), "False");
        assert_eq!(ConditionStatus::Unknown.to_string(), "Unknown");
    }
}
