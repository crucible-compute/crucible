use anyhow::{Context as _, Result};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec, ServiceAccount};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use kube::api::{Api, ObjectMeta, Patch, PatchParams};

use crucible_types::platform::CruciblePlatform;

use crate::context::Context;

const FLINK_OPERATOR_IMAGE: &str = "apache/flink-kubernetes-operator:1.10.0";

pub async fn reconcile(platform: &CruciblePlatform, ctx: &Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = platform.metadata.namespace.as_deref().unwrap_or("default");

    tracing::info!(name, "reconciling Flink Operator");

    let labels = std::collections::BTreeMap::from([
        (
            "app.kubernetes.io/name".to_string(),
            "flink-kubernetes-operator".to_string(),
        ),
        (
            "app.kubernetes.io/managed-by".to_string(),
            "crucible-operator".to_string(),
        ),
        ("crucible.dev/platform".to_string(), name.to_string()),
    ]);

    // Ensure ServiceAccount
    let sa_name = format!("{name}-flink-operator");
    let sa = ServiceAccount {
        metadata: ObjectMeta {
            name: Some(sa_name.clone()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let sa_api: Api<ServiceAccount> = Api::namespaced(ctx.client.clone(), namespace);
    sa_api
        .patch(
            &sa_name,
            &PatchParams::apply("crucible-operator"),
            &Patch::Apply(sa),
        )
        .await
        .context("creating Flink Operator service account")?;

    // Deploy Flink Operator
    let deploy_name = format!("{name}-flink-operator");
    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(deploy_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    service_account_name: Some(sa_name),
                    containers: vec![Container {
                        name: "flink-operator".to_string(),
                        image: Some(FLINK_OPERATOR_IMAGE.to_string()),
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
    .await
    .context("deploying Flink Operator")?;

    Ok(())
}
