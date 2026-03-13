use anyhow::{Context as _, Result};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec, Service, ServicePort, ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use kube::api::{Api, ObjectMeta, Patch, PatchParams};

use crucible_types::platform::CruciblePlatform;

use crate::context::Context;

const HISTORY_SERVER_PORT: i32 = 18080;

pub async fn reconcile(platform: &CruciblePlatform, ctx: &Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = platform.metadata.namespace.as_deref().unwrap_or("default");
    let spec = &platform.spec.history_server;

    if !spec.enabled {
        tracing::info!(name, "History Server disabled, skipping");
        return Ok(());
    }

    let image = spec.image.as_deref().unwrap_or("apache/spark:4.0.0");

    tracing::info!(name, image, "reconciling History Server");

    let labels = std::collections::BTreeMap::from([
        (
            "app.kubernetes.io/name".to_string(),
            "spark-history-server".to_string(),
        ),
        (
            "app.kubernetes.io/managed-by".to_string(),
            "crucible-operator".to_string(),
        ),
        ("crucible.dev/platform".to_string(), name.to_string()),
    ]);

    // Reconcile Deployment
    reconcile_deployment(
        ctx,
        namespace,
        name,
        image,
        &labels,
        &platform.spec.object_store,
    )
    .await
    .context("reconciling History Server deployment")?;

    // Reconcile Service
    reconcile_service(ctx, namespace, name, &labels)
        .await
        .context("reconciling History Server service")?;

    Ok(())
}

async fn reconcile_deployment(
    ctx: &Context,
    namespace: &str,
    platform_name: &str,
    image: &str,
    labels: &std::collections::BTreeMap<String, String>,
    object_store: &crucible_types::platform::ObjectStoreConfig,
) -> Result<()> {
    let deploy_name = format!("{platform_name}-history-server");
    let event_log_dir = format!("s3a://{}/spark-events", object_store.bucket);

    let mut env = vec![EnvVar {
        name: "SPARK_HISTORY_OPTS".to_string(),
        value: Some(format!(
            "-Dspark.history.fs.logDirectory={event_log_dir} \
                 -Dspark.history.store.hybridStore.enabled=true \
                 -Dspark.history.store.hybridStore.diskBackend=ROCKSDB"
        )),
        ..Default::default()
    }];

    if let Some(endpoint) = &object_store.endpoint {
        env.push(EnvVar {
            name: "SPARK_HADOOP_FS_S3A_ENDPOINT".to_string(),
            value: Some(endpoint.clone()),
            ..Default::default()
        });
    }

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
                    labels: Some(labels.clone()),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "history-server".to_string(),
                        image: Some(image.to_string()),
                        args: Some(vec!["/opt/spark/sbin/start-history-server.sh".to_string()]),
                        ports: Some(vec![ContainerPort {
                            container_port: HISTORY_SERVER_PORT,
                            name: Some("http".to_string()),
                            ..Default::default()
                        }]),
                        env: Some(env),
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

async fn reconcile_service(
    ctx: &Context,
    namespace: &str,
    platform_name: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let svc_name = format!("{platform_name}-history-server");

    let service = Service {
        metadata: ObjectMeta {
            name: Some(svc_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels.clone()),
            ports: Some(vec![ServicePort {
                port: HISTORY_SERVER_PORT,
                name: Some("http".to_string()),
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
