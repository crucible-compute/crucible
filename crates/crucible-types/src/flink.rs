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
