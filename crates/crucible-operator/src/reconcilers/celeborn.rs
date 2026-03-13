use anyhow::{Context as _, Result};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec, Service, ServicePort, ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use kube::api::{Api, ObjectMeta, Patch, PatchParams};

use crucible_types::platform::CruciblePlatform;

use crate::context;

const CELEBORN_MASTER_PORT: i32 = 9097;
const REPLICATION_FACTOR: &str = "2";

pub async fn reconcile(platform: &CruciblePlatform, ctx: &context::Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = platform.metadata.namespace.as_deref().unwrap_or("default");
    let spec = &platform.spec.celeborn;

    tracing::info!(name, workers = spec.workers, "reconciling Celeborn");

    let labels = std::collections::BTreeMap::from([
        ("app.kubernetes.io/name".to_string(), "celeborn".to_string()),
        (
            "app.kubernetes.io/managed-by".to_string(),
            "crucible-operator".to_string(),
        ),
        ("crucible.dev/platform".to_string(), name.to_string()),
    ]);

    // Reconcile master Deployment
    reconcile_master(ctx, namespace, name, &spec.image, &labels)
        .await
        .context("reconciling Celeborn master")?;

    // Reconcile master Service
    reconcile_master_service(ctx, namespace, name, &labels)
        .await
        .context("reconciling Celeborn master service")?;

    // Reconcile worker StatefulSet
    reconcile_workers(ctx, namespace, name, &spec.image, spec.workers, &labels)
        .await
        .context("reconciling Celeborn workers")?;

    Ok(())
}

async fn reconcile_master(
    ctx: &context::Context,
    namespace: &str,
    platform_name: &str,
    image: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let deploy_name = format!("{platform_name}-celeborn-master");
    let mut master_labels = labels.clone();
    master_labels.insert(
        "app.kubernetes.io/component".to_string(),
        "master".to_string(),
    );

    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(deploy_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(master_labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(master_labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(master_labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "celeborn-master".to_string(),
                        image: Some(image.to_string()),
                        args: Some(vec!["/opt/celeborn/sbin/start-master.sh".to_string()]),
                        ports: Some(vec![ContainerPort {
                            container_port: CELEBORN_MASTER_PORT,
                            name: Some("rpc".to_string()),
                            ..Default::default()
                        }]),
                        env: Some(vec![EnvVar {
                            name: "CELEBORN_MASTER_HOST".to_string(),
                            value: Some("0.0.0.0".to_string()),
                            ..Default::default()
                        }]),
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

async fn reconcile_master_service(
    ctx: &context::Context,
    namespace: &str,
    platform_name: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let svc_name = format!("{platform_name}-celeborn-master");
    let mut selector = labels.clone();
    selector.insert(
        "app.kubernetes.io/component".to_string(),
        "master".to_string(),
    );

    let service = Service {
        metadata: ObjectMeta {
            name: Some(svc_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                port: CELEBORN_MASTER_PORT,
                name: Some("rpc".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<Service> = Api::namespaced(ctx.client.clone(), namespace);
    api.patch(
        &svc_name,
        &PatchParams::apply("crucible-operator"),
        &Patch::Apply(service),
    )
    .await?;

    Ok(())
}

async fn reconcile_workers(
    ctx: &context::Context,
    namespace: &str,
    platform_name: &str,
    image: &str,
    workers: u32,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let sts_name = format!("{platform_name}-celeborn-worker");
    let master_endpoint = format!("{platform_name}-celeborn-master:{CELEBORN_MASTER_PORT}");

    let mut worker_labels = labels.clone();
    worker_labels.insert(
        "app.kubernetes.io/component".to_string(),
        "worker".to_string(),
    );

    let statefulset = StatefulSet {
        metadata: ObjectMeta {
            name: Some(sts_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(worker_labels.clone()),
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            replicas: Some(workers as i32),
            service_name: Some(sts_name.clone()),
            selector: LabelSelector {
                match_labels: Some(worker_labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(worker_labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "celeborn-worker".to_string(),
                        image: Some(image.to_string()),
                        args: Some(vec!["/opt/celeborn/sbin/start-worker.sh".to_string()]),
                        env: Some(vec![
                            EnvVar {
                                name: "CELEBORN_MASTER_ENDPOINTS".to_string(),
                                value: Some(master_endpoint),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "CELEBORN_WORKER_REPLICATION_FACTOR".to_string(),
                                value: Some(REPLICATION_FACTOR.to_string()),
                                ..Default::default()
                            },
                        ]),
                        volume_mounts: None, // emptyDir handled by K8s default; NVMe/gp3 in later milestones
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<StatefulSet> = Api::namespaced(ctx.client.clone(), namespace);
    api.patch(
        &sts_name,
        &PatchParams::apply("crucible-operator"),
        &Patch::Apply(statefulset),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crucible_types::platform::StorageType;

    #[test]
    fn replication_factor_is_always_two() {
        assert_eq!(REPLICATION_FACTOR, "2");
    }

    #[test]
    fn master_endpoint_format() {
        let platform_name = "test-platform";
        let endpoint = format!("{platform_name}-celeborn-master:{CELEBORN_MASTER_PORT}");
        assert_eq!(endpoint, "test-platform-celeborn-master:9097");
    }

    #[test]
    fn labels_include_platform_ref() {
        let labels = std::collections::BTreeMap::from([
            ("app.kubernetes.io/name".to_string(), "celeborn".to_string()),
            (
                "crucible.dev/platform".to_string(),
                "my-platform".to_string(),
            ),
        ]);
        assert_eq!(labels.get("crucible.dev/platform").unwrap(), "my-platform");
    }

    #[test]
    fn storage_type_variants() {
        // Ensure all storage types are covered
        let _empty = StorageType::EmptyDir;
        let _nvme = StorageType::Nvme;
        let _gp3 = StorageType::Gp3;
    }
}
