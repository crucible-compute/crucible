use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::JobPhase;

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "crucible.dev",
    version = "v1alpha1",
    kind = "CrucibleFlinkJob",
    namespaced,
    status = "CrucibleFlinkJobStatus",
    shortname = "cfj"
)]
pub struct CrucibleFlinkJobSpec {
    pub jar: String,
    pub entry_class: String,
    pub parallelism: Option<u32>,
    pub task_managers: Option<u32>,
    pub tenant: String,
    #[serde(default)]
    pub flink_config: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CrucibleFlinkJobStatus {
    pub phase: Option<JobPhase>,
    pub job_manager_pod: Option<String>,
    pub ui_url: Option<String>,
    pub error: Option<String>,
    pub last_savepoint_path: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_serialization_round_trip() {
        let spec = CrucibleFlinkJobSpec {
            jar: "s3://bucket/flink-app.jar".to_string(),
            entry_class: "com.example.StreamingJob".to_string(),
            parallelism: Some(8),
            task_managers: Some(2),
            tenant: "team-b".to_string(),
            flink_config: [("state.backend".to_string(), "rocksdb".to_string())]
                .into_iter()
                .collect(),
            labels: Default::default(),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: CrucibleFlinkJobSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.jar, "s3://bucket/flink-app.jar");
        assert_eq!(deserialized.tenant, "team-b");
        assert_eq!(deserialized.parallelism, Some(8));
        assert!(deserialized.flink_config.contains_key("state.backend"));
    }

    #[test]
    fn status_round_trip_with_savepoint() {
        let status = CrucibleFlinkJobStatus {
            phase: Some(JobPhase::Suspended),
            job_manager_pod: Some("flink-jm-0".to_string()),
            ui_url: None,
            error: None,
            last_savepoint_path: Some("s3://bucket/flink-savepoints/job-1/sp-1".to_string()),
            start_time: Some("2026-01-01T00:00:00Z".to_string()),
            end_time: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: CrucibleFlinkJobStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.phase, Some(JobPhase::Suspended));
        assert!(deserialized.last_savepoint_path.is_some());
    }

    #[test]
    fn minimal_spec_deserializes() {
        let json = r#"{"jar":"app.jar","entry_class":"Main","tenant":"default"}"#;
        let spec: CrucibleFlinkJobSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.entry_class, "Main");
        assert!(spec.parallelism.is_none());
        assert!(spec.flink_config.is_empty());
    }
}
