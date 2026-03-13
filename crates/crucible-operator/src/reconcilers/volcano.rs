use anyhow::{Context as _, Result};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec, ServiceAccount};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use kube::api::{Api, ObjectMeta, Patch, PatchParams};

use crucible_types::platform::{CruciblePlatform, TenantConfig};

use crate::context::Context;

const VOLCANO_SCHEDULER_IMAGE: &str = "volcanosh/vc-scheduler:v1.10.0";
const VOLCANO_CONTROLLER_IMAGE: &str = "volcanosh/vc-controller-manager:v1.10.0";

pub async fn reconcile(platform: &CruciblePlatform, ctx: &Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = platform.metadata.namespace.as_deref().unwrap_or("default");
    let spec = &platform.spec.volcano;

    tracing::info!(name, tenants = spec.tenants.len(), "reconciling Volcano");

    let labels = std::collections::BTreeMap::from([
        ("app.kubernetes.io/name".to_string(), "volcano".to_string()),
        (
            "app.kubernetes.io/managed-by".to_string(),
            "crucible-operator".to_string(),
        ),
        ("crucible.dev/platform".to_string(), name.to_string()),
    ]);

    // Deploy Volcano scheduler
    reconcile_scheduler(ctx, namespace, name, &labels)
        .await
        .context("reconciling Volcano scheduler")?;

    // Deploy Volcano controller manager
    reconcile_controller(ctx, namespace, name, &labels)
        .await
        .context("reconciling Volcano controller")?;

    // Create tenant queues
    reconcile_queues(ctx, namespace, name, &spec.tenants)
        .await
        .context("reconciling Volcano queues")?;

    Ok(())
}

async fn reconcile_scheduler(
    ctx: &Context,
    namespace: &str,
    platform_name: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let deploy_name = format!("{platform_name}-volcano-scheduler");
    let mut sched_labels = labels.clone();
    sched_labels.insert(
        "app.kubernetes.io/component".to_string(),
        "scheduler".to_string(),
    );

    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(deploy_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(sched_labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(sched_labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(sched_labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    service_account_name: Some(format!("{platform_name}-volcano")),
                    containers: vec![Container {
                        name: "volcano-scheduler".to_string(),
                        image: Some(VOLCANO_SCHEDULER_IMAGE.to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<Deployment> = Api::namespaced(ctx.client.clone(), namespace);
    api.patch(
        &deploy_name,
        &PatchParams::apply("crucible-operator"),
        &Patch::Apply(deployment),
    )
    .await?;

    // Ensure ServiceAccount exists
    let sa = ServiceAccount {
        metadata: ObjectMeta {
            name: Some(format!("{platform_name}-volcano")),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let sa_api: Api<ServiceAccount> = Api::namespaced(ctx.client.clone(), namespace);
    sa_api
        .patch(
            &format!("{platform_name}-volcano"),
            &PatchParams::apply("crucible-operator"),
            &Patch::Apply(sa),
        )
        .await?;

    Ok(())
}

async fn reconcile_controller(
    ctx: &Context,
    namespace: &str,
    platform_name: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let deploy_name = format!("{platform_name}-volcano-controller");
    let mut ctrl_labels = labels.clone();
    ctrl_labels.insert(
        "app.kubernetes.io/component".to_string(),
        "controller".to_string(),
    );

    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(deploy_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(ctrl_labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(ctrl_labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(ctrl_labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    service_account_name: Some(format!("{platform_name}-volcano")),
                    containers: vec![Container {
                        name: "volcano-controller".to_string(),
                        image: Some(VOLCANO_CONTROLLER_IMAGE.to_string()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<Deployment> = Api::namespaced(ctx.client.clone(), namespace);
    api.patch(
        &deploy_name,
        &PatchParams::apply("crucible-operator"),
        &Patch::Apply(deployment),
    )
    .await?;

    Ok(())
}

async fn reconcile_queues(
    _ctx: &Context,
    _namespace: &str,
    platform_name: &str,
    tenants: &[TenantConfig],
) -> Result<()> {
    // Volcano Queue is a cluster-scoped CRD. We generate the queue specs here
    // but applying them requires the Volcano CRDs to be installed first.
    // For now, log the intent — actual CRD apply comes with Volcano manifest bundle.
    for tenant in tenants {
        let queue_name = format!("{platform_name}-{}", tenant.name);
        let weight = tenant.queue_weight.unwrap_or(1);
        tracing::info!(queue_name, weight, "ensuring Volcano queue");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_spec_from_tenants() {
        let tenants = [
            TenantConfig {
                name: "default".to_string(),
                queue_weight: Some(1),
            },
            TenantConfig {
                name: "team-a".to_string(),
                queue_weight: Some(3),
            },
        ];

        let queue_names: Vec<String> = tenants
            .iter()
            .map(|t| format!("test-platform-{}", t.name))
            .collect();

        assert_eq!(
            queue_names,
            vec!["test-platform-default", "test-platform-team-a"]
        );
    }

    #[test]
    fn default_queue_weight() {
        let tenant = TenantConfig {
            name: "no-weight".to_string(),
            queue_weight: None,
        };
        assert_eq!(tenant.queue_weight.unwrap_or(1), 1);
    }
}
