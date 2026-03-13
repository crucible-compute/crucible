use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::JobPhase;

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "crucible.dev",
    version = "v1alpha1",
    kind = "CrucibleSparkJob",
    namespaced,
    status = "CrucibleSparkJobStatus",
    shortname = "csj"
)]
#[serde(rename_all = "camelCase")]
pub struct CrucibleSparkJobSpec {
    pub jar: String,
    pub class: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub tenant: String,
    pub executors: Option<u32>,
    pub driver_resources: Option<ResourceSpec>,
    pub executor_resources: Option<ResourceSpec>,
    #[serde(default)]
    pub spark_config: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceSpec {
    pub cpu: Option<String>,
    pub memory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct CrucibleSparkJobStatus {
    pub phase: Option<JobPhase>,
    pub driver_pod: Option<String>,
    #[serde(default)]
    pub executor_pods: Vec<String>,
    pub ui_url: Option<String>,
    pub error: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_serialization_round_trip() {
        let spec = CrucibleSparkJobSpec {
            jar: "s3://bucket/app.jar".to_string(),
            class: "com.example.SparkPi".to_string(),
            args: vec!["100".to_string()],
            tenant: "team-a".to_string(),
            executors: Some(4),
            driver_resources: Some(ResourceSpec {
                cpu: Some("2".to_string()),
                memory: Some("4Gi".to_string()),
            }),
            executor_resources: None,
            spark_config: [("spark.executor.cores".to_string(), "4".to_string())]
                .into_iter()
                .collect(),
            labels: Default::default(),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: CrucibleSparkJobSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.jar, "s3://bucket/app.jar");
        assert_eq!(deserialized.tenant, "team-a");
        assert_eq!(deserialized.executors, Some(4));
        assert_eq!(deserialized.args.len(), 1);
        assert!(
            deserialized
                .spark_config
                .contains_key("spark.executor.cores")
        );
    }

    #[test]
    fn status_round_trip() {
        let status = CrucibleSparkJobStatus {
            phase: Some(JobPhase::Running),
            driver_pod: Some("spark-pi-driver".to_string()),
            executor_pods: vec!["spark-pi-exec-0".to_string()],
            ui_url: Some("http://spark-pi-driver:4040".to_string()),
            error: None,
            start_time: Some("2026-01-01T00:00:00Z".to_string()),
            end_time: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: CrucibleSparkJobStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.phase, Some(JobPhase::Running));
        assert_eq!(deserialized.executor_pods.len(), 1);
    }

    #[test]
    fn minimal_spec_deserializes() {
        let json = r#"{"jar":"app.jar","class":"Main","tenant":"default"}"#;
        // Note: camelCase fields like sparkConfig/driverResources default to empty/None
        let spec: CrucibleSparkJobSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.jar, "app.jar");
        assert!(spec.args.is_empty());
        assert!(spec.executors.is_none());
        assert!(spec.spark_config.is_empty());
    }
}
