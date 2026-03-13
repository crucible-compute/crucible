use std::sync::Arc;

use anyhow::Result;
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::controller::Action;
use serde_json::json;
use tokio::time::Duration;

use crucible_types::platform::{
    ConditionStatus, ConditionType, CruciblePlatform, CruciblePlatformStatus, PlatformCondition,
    PlatformPhase,
};

use crate::context::Context;
use crate::reconcilers::{celeborn, flink_operator, history_server, volcano};

/// Top-level reconciler for CruciblePlatform.
/// Dispatches to sub-reconcilers for each component and aggregates status.
pub async fn reconcile(
    platform: Arc<CruciblePlatform>,
    ctx: Arc<Context>,
) -> Result<Action, kube::Error> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = platform.metadata.namespace.as_deref().unwrap_or("default");

    tracing::info!(name, namespace, "reconciling CruciblePlatform");

    let api: Api<CruciblePlatform> = Api::namespaced(ctx.client.clone(), namespace);

    // Run sub-reconcilers concurrently, collecting results.
    let (celeborn_result, volcano_result, history_result, flink_result) = tokio::join!(
        celeborn::reconcile(&platform, &ctx),
        volcano::reconcile(&platform, &ctx),
        history_server::reconcile(&platform, &ctx),
        flink_operator::reconcile(&platform, &ctx),
    );

    // Build conditions from sub-reconciler results.
    let conditions = vec![
        result_to_condition(ConditionType::CelebornReady, &celeborn_result),
        result_to_condition(ConditionType::VolcanoReady, &volcano_result),
        result_to_condition(ConditionType::HistoryServerReady, &history_result),
        result_to_condition(ConditionType::FlinkOperatorReady, &flink_result),
    ];

    // Determine overall phase from conditions.
    let phase = compute_phase(&conditions);

    let status = CruciblePlatformStatus {
        phase: Some(phase),
        conditions,
    };

    // Patch the status subresource.
    let patch = json!({ "status": status });
    api.patch_status(
        name,
        &PatchParams::apply("crucible-operator"),
        &Patch::Merge(&patch),
    )
    .await?;

    // Requeue: faster when deploying, slower when stable.
    let requeue = if status_all_ready(&status) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(10)
    };

    Ok(Action::requeue(requeue))
}

/// Called when a reconcile error occurs.
pub fn error_policy(
    _platform: Arc<CruciblePlatform>,
    error: &kube::Error,
    _ctx: Arc<Context>,
) -> Action {
    tracing::error!(%error, "reconcile error");
    Action::requeue(Duration::from_secs(30))
}

fn result_to_condition(
    condition_type: ConditionType,
    result: &Result<(), anyhow::Error>,
) -> PlatformCondition {
    match result {
        Ok(()) => PlatformCondition {
            r#type: condition_type,
            status: ConditionStatus::True,
            message: None,
            last_transition_time: Some(chrono::Utc::now().to_rfc3339()),
        },
        Err(e) => PlatformCondition {
            r#type: condition_type,
            status: ConditionStatus::False,
            message: Some(e.to_string()),
            last_transition_time: Some(chrono::Utc::now().to_rfc3339()),
        },
    }
}

fn compute_phase(conditions: &[PlatformCondition]) -> PlatformPhase {
    let all_ready = conditions.iter().all(|c| c.status == ConditionStatus::True);
    let any_failed = conditions
        .iter()
        .any(|c| c.status == ConditionStatus::False);

    if all_ready {
        PlatformPhase::Ready
    } else if any_failed {
        PlatformPhase::Degraded
    } else {
        PlatformPhase::Deploying
    }
}

fn status_all_ready(status: &CruciblePlatformStatus) -> bool {
    status.phase == Some(PlatformPhase::Ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ready_gives_ready_phase() {
        let conditions = vec![
            PlatformCondition {
                r#type: ConditionType::CelebornReady,
                status: ConditionStatus::True,
                message: None,
                last_transition_time: None,
            },
            PlatformCondition {
                r#type: ConditionType::VolcanoReady,
                status: ConditionStatus::True,
                message: None,
                last_transition_time: None,
            },
            PlatformCondition {
                r#type: ConditionType::HistoryServerReady,
                status: ConditionStatus::True,
                message: None,
                last_transition_time: None,
            },
            PlatformCondition {
                r#type: ConditionType::FlinkOperatorReady,
                status: ConditionStatus::True,
                message: None,
                last_transition_time: None,
            },
        ];
        assert_eq!(compute_phase(&conditions), PlatformPhase::Ready);
    }

    #[test]
    fn one_failed_gives_degraded() {
        let conditions = vec![
            PlatformCondition {
                r#type: ConditionType::CelebornReady,
                status: ConditionStatus::True,
                message: None,
                last_transition_time: None,
            },
            PlatformCondition {
                r#type: ConditionType::VolcanoReady,
                status: ConditionStatus::False,
                message: Some("failed to deploy".to_string()),
                last_transition_time: None,
            },
        ];
        assert_eq!(compute_phase(&conditions), PlatformPhase::Degraded);
    }

    #[test]
    fn failed_sub_reconciler_does_not_block_others() {
        // Simulate: Flink sub-reconciler fails, but Celeborn succeeds.
        let celeborn_result: Result<(), anyhow::Error> = Ok(());
        let flink_result: Result<(), anyhow::Error> =
            Err(anyhow::anyhow!("flink operator install failed"));

        let c1 = result_to_condition(ConditionType::CelebornReady, &celeborn_result);
        let c2 = result_to_condition(ConditionType::FlinkOperatorReady, &flink_result);

        assert_eq!(c1.status, ConditionStatus::True);
        assert_eq!(c2.status, ConditionStatus::False);
        assert!(
            c2.message
                .unwrap()
                .contains("flink operator install failed")
        );
    }
}
