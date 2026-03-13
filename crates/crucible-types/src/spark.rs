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
