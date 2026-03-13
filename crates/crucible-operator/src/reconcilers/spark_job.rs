use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use k8s_openapi::api::core::v1::{Container, EnvVar, Pod, PodSpec, ServiceAccount};
use k8s_openapi::api::rbac::v1::{PolicyRule, Role, RoleBinding, RoleRef, Subject};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{Api, DeleteParams, ListParams, ObjectMeta, Patch, PatchParams};
use kube::Resource;
use kube::runtime::controller::Action;
use serde_json::json;
use tokio::time::Duration;

use crucible_types::platform::CruciblePlatform;
use crucible_types::spark::{CrucibleSparkJob, CrucibleSparkJobStatus};
use crucible_types::state::JobPhase;

use crate::context::Context;

const FIELD_MANAGER: &str = "crucible-operator";
const FINALIZER: &str = "crucible.dev/spark-job-cleanup";
const CELEBORN_MASTER_PORT: i32 = 9097;
const DEFAULT_SPARK_IMAGE: &str = "crucible-spark:latest";
const DEFAULT_DRIVER_CPU: &str = "1";
const DEFAULT_DRIVER_MEMORY: &str = "1Gi";
const DEFAULT_EXECUTOR_CPU: &str = "1";
const DEFAULT_EXECUTOR_MEMORY: &str = "1Gi";
const DEFAULT_EXECUTORS: u32 = 1;

/// Top-level reconciler for CrucibleSparkJob.
pub async fn reconcile(
    job: Arc<CrucibleSparkJob>,
    ctx: Arc<Context>,
) -> Result<Action, kube::Error> {
    let name = job.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = job.metadata.namespace.as_deref().unwrap_or("default");

    tracing::info!(name, namespace, "reconciling CrucibleSparkJob");

    let api: Api<CrucibleSparkJob> = Api::namespaced(ctx.client.clone(), namespace);

    // Handle deletion — run cleanup then remove finalizer.
    if job.metadata.deletion_timestamp.is_some() {
        tracing::info!(name, "CrucibleSparkJob being deleted, running cleanup");
        if let Err(e) = handle_delete(&job, &ctx).await {
            tracing::error!(%e, name, "cleanup failed during delete");
        }
        // Remove finalizer to let K8s complete the deletion.
        let patch = json!({
            "metadata": {
                "finalizers": job.metadata.finalizers.as_ref()
                    .map(|f| f.iter().filter(|fin| fin.as_str() != FINALIZER).cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            }
        });
        api.patch(name, &PatchParams::default(), &Patch::Merge(&patch)).await?;
        return Ok(Action::await_change());
    }

    // Ensure finalizer is set.
    if !job
        .metadata
        .finalizers
        .as_ref()
        .is_some_and(|f| f.iter().any(|fin| fin == FINALIZER))
    {
        let patch = json!({
            "metadata": {
                "finalizers": [FINALIZER]
            }
        });
        api.patch(name, &PatchParams::apply(FIELD_MANAGER), &Patch::Merge(&patch)).await?;
        // Requeue — the patched object will trigger a new reconcile.
        return Ok(Action::requeue(Duration::from_millis(100)));
    }

    let current_phase = job
        .status
        .as_ref()
        .and_then(|s| s.phase.as_ref())
        .cloned();

    // If already terminal, nothing to do.
    if current_phase
        .as_ref()
        .is_some_and(|p| p.is_terminal())
    {
        return Ok(Action::await_change());
    }

    match do_reconcile(&job, &ctx).await {
        Ok(new_status) => {
            let patch = json!({ "status": new_status });
            api.patch_status(
                name,
                &PatchParams::apply(FIELD_MANAGER),
                &Patch::Merge(&patch),
            )
            .await?;

            let requeue = if new_status
                .phase
                .as_ref()
                .is_some_and(|p| p.is_terminal())
            {
                Action::await_change()
            } else {
                Action::requeue(Duration::from_secs(5))
            };
            Ok(requeue)
        }
        Err(e) => {
            tracing::error!(%e, name, "spark job reconcile failed");
            let error_status = CrucibleSparkJobStatus {
                phase: Some(JobPhase::Failed),
                error: Some(e.to_string()),
                end_time: Some(chrono::Utc::now().to_rfc3339()),
                ..job.status.clone().unwrap_or_default()
            };
            let patch = json!({ "status": error_status });
            api.patch_status(
                name,
                &PatchParams::apply(FIELD_MANAGER),
                &Patch::Merge(&patch),
            )
            .await?;
            Ok(Action::requeue(Duration::from_secs(30)))
        }
    }
}

pub fn error_policy(
    _job: Arc<CrucibleSparkJob>,
    error: &kube::Error,
    _ctx: Arc<Context>,
) -> Action {
    tracing::error!(%error, "spark job reconcile error");
    Action::requeue(Duration::from_secs(30))
}

/// Build an OwnerReference pointing to the given CrucibleSparkJob.
fn owner_ref(job: &CrucibleSparkJob) -> OwnerReference {
    OwnerReference {
        api_version: CrucibleSparkJob::api_version(&()).to_string(),
        kind: CrucibleSparkJob::kind(&()).to_string(),
        name: job.metadata.name.clone().unwrap_or_default(),
        uid: job.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }
}

/// Core reconciliation logic. Returns the desired status.
async fn do_reconcile(
    job: &CrucibleSparkJob,
    ctx: &Context,
) -> Result<CrucibleSparkJobStatus> {
    let name = job.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = job.metadata.namespace.as_deref().unwrap_or("default");
    let spec = &job.spec;
    let oref = owner_ref(job);

    let current_phase = job
        .status
        .as_ref()
        .and_then(|s| s.phase.as_ref())
        .cloned()
        .unwrap_or(JobPhase::Submitted);

    let mut status = job.status.clone().unwrap_or_default();

    // Look up the CruciblePlatform in this namespace for Celeborn/Volcano config.
    let platform = find_platform(ctx, namespace)
        .await
        .context("looking up CruciblePlatform")?;

    let platform_name = platform
        .metadata
        .name
        .as_deref()
        .unwrap_or("unknown");

    match current_phase {
        JobPhase::Submitted => {
            // Ensure driver ServiceAccount + RBAC exists (for Spark-on-K8s executor creation).
            ensure_driver_rbac(ctx, namespace, name, &oref)
                .await
                .context("creating driver RBAC")?;

            // Log Volcano PodGroup intent (actual CRD apply requires Volcano installed).
            log_podgroup_intent(name, &spec.tenant, spec.executors.unwrap_or(DEFAULT_EXECUTORS));

            // Create driver pod.
            let driver_pod_name = format!("{name}-driver");
            let spark_config = build_spark_config(spec, platform_name, namespace, name);

            let driver_pod = build_driver_pod(
                name,
                namespace,
                &driver_pod_name,
                spec,
                &spark_config,
                &oref,
            );

            let pod_api: Api<Pod> = Api::namespaced(ctx.client.clone(), namespace);
            pod_api
                .patch(
                    &driver_pod_name,
                    &PatchParams::apply(FIELD_MANAGER),
                    &Patch::Apply(driver_pod),
                )
                .await
                .context("creating driver pod")?;

            status.phase = Some(JobPhase::Submitted);
            status.driver_pod = Some(driver_pod_name);
            status.start_time = Some(chrono::Utc::now().to_rfc3339());
        }
        JobPhase::Running => {
            // Check driver pod status.
            let default_driver_name = format!("{name}-driver");
            let driver_pod_name = status
                .driver_pod
                .as_deref()
                .unwrap_or(&default_driver_name);

            let pod_api: Api<Pod> = Api::namespaced(ctx.client.clone(), namespace);
            match pod_api.get(driver_pod_name).await {
                Ok(pod) => {
                    let pod_phase = pod
                        .status
                        .as_ref()
                        .and_then(|s| s.phase.as_deref())
                        .unwrap_or("Unknown");

                    match pod_phase {
                        "Succeeded" => {
                            status.phase = Some(JobPhase::Completed);
                            status.end_time = Some(chrono::Utc::now().to_rfc3339());
                        }
                        "Failed" => {
                            let message = extract_pod_error(&pod);
                            let enriched = enrich_error(ctx, namespace, platform_name, &message).await;
                            status.phase = Some(JobPhase::Failed);
                            status.error = Some(enriched);
                            status.end_time = Some(chrono::Utc::now().to_rfc3339());
                        }
                        _ => {
                            // Still running, update executor pod list.
                            status.executor_pods = list_executor_pods(ctx, namespace, name).await?;
                        }
                    }
                }
                Err(kube::Error::Api(ref resp)) if resp.code == 404 => {
                    // Driver pod gone — failed.
                    status.phase = Some(JobPhase::Failed);
                    status.error = Some("driver pod not found".to_string());
                    status.end_time = Some(chrono::Utc::now().to_rfc3339());
                }
                Err(e) => return Err(e.into()),
            }
        }
        _ => {}
    }

    // Transition Submitted → Running if driver pod is running.
    if status.phase == Some(JobPhase::Submitted)
        && let Some(ref driver_pod_name) = status.driver_pod
    {
        let pod_api: Api<Pod> = Api::namespaced(ctx.client.clone(), namespace);
        if let Ok(pod) = pod_api.get(driver_pod_name).await {
            let pod_phase = pod
                .status
                .as_ref()
                .and_then(|s| s.phase.as_deref())
                .unwrap_or("Pending");

            match pod_phase {
                "Running" => {
                    status.phase = Some(JobPhase::Running);
                    status.ui_url = Some(format!(
                        "http://{driver_pod_name}.{namespace}:4040"
                    ));
                }
                "Succeeded" => {
                    status.phase = Some(JobPhase::Completed);
                    status.end_time = Some(chrono::Utc::now().to_rfc3339());
                }
                "Failed" => {
                    let message = extract_pod_error(&pod);
                    let enriched = enrich_error(ctx, namespace, platform_name, &message).await;
                    status.phase = Some(JobPhase::Failed);
                    status.error = Some(enriched);
                    status.end_time = Some(chrono::Utc::now().to_rfc3339());
                }
                _ => {} // Still pending.
            }
        }
    }

    Ok(status)
}

/// Find the CruciblePlatform in the given namespace.
/// Expects exactly one platform per namespace.
async fn find_platform(ctx: &Context, namespace: &str) -> Result<CruciblePlatform> {
    let api: Api<CruciblePlatform> = Api::namespaced(ctx.client.clone(), namespace);
    let platforms = api
        .list(&ListParams::default())
        .await
        .context("listing CruciblePlatforms")?;

    platforms
        .items
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no CruciblePlatform found in namespace {namespace}"))
}

/// Build the full Spark configuration including Celeborn shuffle settings.
pub fn build_spark_config(
    spec: &crucible_types::spark::CrucibleSparkJobSpec,
    platform_name: &str,
    namespace: &str,
    job_name: &str,
) -> BTreeMap<String, String> {
    let celeborn_endpoint = format!(
        "{platform_name}-celeborn-master.{namespace}.svc:{CELEBORN_MASTER_PORT}"
    );

    let mut config = BTreeMap::new();

    // Celeborn shuffle manager config.
    config.insert(
        "spark.shuffle.manager".to_string(),
        "org.apache.celeborn.spark.CelebornShuffleManager".to_string(),
    );
    config.insert(
        "spark.celeborn.master.endpoints".to_string(),
        celeborn_endpoint,
    );
    config.insert(
        "spark.celeborn.client.spark.shuffle.writer".to_string(),
        "hash".to_string(),
    );

    // Event log config.
    config.insert("spark.eventLog.enabled".to_string(), "true".to_string());
    config.insert(
        "spark.eventLog.dir".to_string(),
        "s3a://crucible-data/spark-events".to_string(),
    );

    // Spark-on-K8s executor container image and service account.
    let image_pull_policy = std::env::var("IMAGE_PULL_POLICY")
        .unwrap_or_else(|_| "IfNotPresent".to_string());
    config.insert(
        "spark.kubernetes.container.image".to_string(),
        DEFAULT_SPARK_IMAGE.to_string(),
    );
    config.insert(
        "spark.kubernetes.container.image.pullPolicy".to_string(),
        image_pull_policy,
    );
    config.insert(
        "spark.kubernetes.authenticate.driver.serviceAccountName".to_string(),
        driver_sa_name(job_name),
    );

    // Decommissioning config for graceful executor shutdown.
    config.insert(
        "spark.decommission.enabled".to_string(),
        "true".to_string(),
    );
    config.insert(
        "spark.storage.decommission.rddBlocks.enabled".to_string(),
        "true".to_string(),
    );
    config.insert(
        "spark.storage.decommission.shuffleBlocks.enabled".to_string(),
        "true".to_string(),
    );

    // User-provided overrides (last, so they win).
    for (k, v) in &spec.spark_config {
        config.insert(k.clone(), v.clone());
    }

    config
}

/// Build the driver Pod spec.
fn build_driver_pod(
    job_name: &str,
    namespace: &str,
    driver_pod_name: &str,
    spec: &crucible_types::spark::CrucibleSparkJobSpec,
    spark_config: &BTreeMap<String, String>,
    owner_ref: &OwnerReference,
) -> Pod {
    let labels = job_labels(job_name, "driver");

    let driver_cpu = spec
        .driver_resources
        .as_ref()
        .and_then(|r| r.cpu.as_deref())
        .unwrap_or(DEFAULT_DRIVER_CPU);
    let driver_memory = spec
        .driver_resources
        .as_ref()
        .and_then(|r| r.memory.as_deref())
        .unwrap_or(DEFAULT_DRIVER_MEMORY);

    let sa_name = driver_sa_name(job_name);

    // Build spark-submit arguments — Spark-on-K8s mode.
    let mut spark_args = vec![
        "/opt/spark/bin/spark-submit".to_string(),
        "--master".to_string(),
        "k8s://https://kubernetes.default.svc".to_string(),
        "--deploy-mode".to_string(),
        "client".to_string(),
        "--class".to_string(),
        spec.class.clone(),
        "--driver-memory".to_string(),
        driver_memory.to_string(),
    ];

    // Add spark config as --conf arguments.
    for (k, v) in spark_config {
        spark_args.push("--conf".to_string());
        spark_args.push(format!("{k}={v}"));
    }

    // Executor config.
    let num_executors = spec.executors.unwrap_or(DEFAULT_EXECUTORS);
    let executor_cpu = spec
        .executor_resources
        .as_ref()
        .and_then(|r| r.cpu.as_deref())
        .unwrap_or(DEFAULT_EXECUTOR_CPU);
    let executor_memory = spec
        .executor_resources
        .as_ref()
        .and_then(|r| r.memory.as_deref())
        .unwrap_or(DEFAULT_EXECUTOR_MEMORY);

    spark_args.push("--conf".to_string());
    spark_args.push(format!("spark.executor.instances={num_executors}"));
    spark_args.push("--conf".to_string());
    spark_args.push(format!("spark.executor.cores={executor_cpu}"));
    spark_args.push("--conf".to_string());
    spark_args.push(format!("spark.executor.memory={executor_memory}"));

    // K8s executor config for Spark-on-K8s.
    spark_args.push("--conf".to_string());
    spark_args.push(format!(
        "spark.kubernetes.namespace={namespace}"
    ));
    spark_args.push("--conf".to_string());
    spark_args.push(format!(
        "spark.kubernetes.executor.label.crucible.dev/job={job_name}"
    ));
    spark_args.push("--conf".to_string());
    spark_args.push(
        "spark.kubernetes.executor.label.crucible.dev/role=executor".to_string(),
    );

    // JAR and user arguments.
    spark_args.push(spec.jar.clone());
    spark_args.extend(spec.args.iter().cloned());

    let env_vars = vec![EnvVar {
        name: "SPARK_HOME".to_string(),
        value: Some("/opt/spark".to_string()),
        ..Default::default()
    }];

    let image_pull_policy = std::env::var("IMAGE_PULL_POLICY")
        .unwrap_or_else(|_| "IfNotPresent".to_string());

    let mut resource_reqs = k8s_openapi::api::core::v1::ResourceRequirements::default();
    let mut requests = BTreeMap::new();
    requests.insert(
        "cpu".to_string(),
        k8s_openapi::apimachinery::pkg::api::resource::Quantity(driver_cpu.to_string()),
    );
    requests.insert(
        "memory".to_string(),
        k8s_openapi::apimachinery::pkg::api::resource::Quantity(driver_memory.to_string()),
    );
    resource_reqs.requests = Some(requests);

    Pod {
        metadata: ObjectMeta {
            name: Some(driver_pod_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            owner_references: Some(vec![owner_ref.clone()]),
            ..Default::default()
        },
        spec: Some(PodSpec {
            service_account_name: Some(sa_name),
            restart_policy: Some("Never".to_string()),
            containers: vec![Container {
                name: "spark-driver".to_string(),
                image: Some(DEFAULT_SPARK_IMAGE.to_string()),
                image_pull_policy: Some(image_pull_policy),
                command: Some(vec!["/bin/bash".to_string(), "-c".to_string()]),
                args: Some(vec![spark_args.join(" ")]),
                env: Some(env_vars),
                resources: Some(resource_reqs),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Standard labels for Spark job resources.
pub fn job_labels(job_name: &str, role: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "app.kubernetes.io/name".to_string(),
            "crucible-spark-job".to_string(),
        ),
        (
            "app.kubernetes.io/managed-by".to_string(),
            FIELD_MANAGER.to_string(),
        ),
        ("crucible.dev/job".to_string(), job_name.to_string()),
        ("crucible.dev/role".to_string(), role.to_string()),
    ])
}

/// ServiceAccount name for the Spark driver pod.
fn driver_sa_name(job_name: &str) -> String {
    format!("{job_name}-driver")
}

/// Ensure the driver ServiceAccount, Role, and RoleBinding exist so the driver
/// can create/watch/delete executor pods via Spark-on-K8s.
async fn ensure_driver_rbac(ctx: &Context, namespace: &str, job_name: &str, owner_ref: &OwnerReference) -> Result<()> {
    let sa_name = driver_sa_name(job_name);
    let role_name = format!("{job_name}-spark-role");
    let binding_name = format!("{job_name}-spark-binding");

    let labels = job_labels(job_name, "driver-rbac");

    // ServiceAccount
    let sa = ServiceAccount {
        metadata: ObjectMeta {
            name: Some(sa_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner_ref.clone()]),
            ..Default::default()
        },
        ..Default::default()
    };
    let sa_api: Api<ServiceAccount> = Api::namespaced(ctx.client.clone(), namespace);
    sa_api
        .patch(&sa_name, &PatchParams::apply(FIELD_MANAGER), &Patch::Apply(sa))
        .await
        .context("creating driver ServiceAccount")?;

    // Role — driver needs to manage executor pods and services.
    let role = Role {
        metadata: ObjectMeta {
            name: Some(role_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner_ref.clone()]),
            ..Default::default()
        },
        rules: Some(vec![
            PolicyRule {
                api_groups: Some(vec!["".to_string()]),
                resources: Some(vec![
                    "pods".to_string(),
                    "services".to_string(),
                    "configmaps".to_string(),
                ]),
                verbs: vec![
                    "get".to_string(),
                    "list".to_string(),
                    "watch".to_string(),
                    "create".to_string(),
                    "update".to_string(),
                    "patch".to_string(),
                    "delete".to_string(),
                ],
                ..Default::default()
            },
        ]),
    };
    let role_api: Api<Role> = Api::namespaced(ctx.client.clone(), namespace);
    role_api
        .patch(&role_name, &PatchParams::apply(FIELD_MANAGER), &Patch::Apply(role))
        .await
        .context("creating driver Role")?;

    // RoleBinding
    let binding = RoleBinding {
        metadata: ObjectMeta {
            name: Some(binding_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            owner_references: Some(vec![owner_ref.clone()]),
            ..Default::default()
        },
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".to_string(),
            kind: "Role".to_string(),
            name: role_name,
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".to_string(),
            name: sa_name,
            namespace: Some(namespace.to_string()),
            ..Default::default()
        }]),
    };
    let binding_api: Api<RoleBinding> = Api::namespaced(ctx.client.clone(), namespace);
    binding_api
        .patch(
            &binding_name,
            &PatchParams::apply(FIELD_MANAGER),
            &Patch::Apply(binding),
        )
        .await
        .context("creating driver RoleBinding")?;

    Ok(())
}

/// List executor pods for a job by label selector.
async fn list_executor_pods(
    ctx: &Context,
    namespace: &str,
    job_name: &str,
) -> Result<Vec<String>> {
    let pod_api: Api<Pod> = Api::namespaced(ctx.client.clone(), namespace);
    let lp = ListParams::default()
        .labels(&format!("crucible.dev/job={job_name},crucible.dev/role=executor"));
    let pods = pod_api.list(&lp).await?;
    Ok(pods
        .items
        .iter()
        .filter_map(|p| p.metadata.name.clone())
        .collect())
}

/// Extract error message from a failed pod.
fn extract_pod_error(pod: &Pod) -> String {
    pod.status
        .as_ref()
        .and_then(|s| {
            s.container_statuses.as_ref().and_then(|cs| {
                cs.iter().find_map(|c| {
                    c.state.as_ref().and_then(|state| {
                        state
                            .terminated
                            .as_ref()
                            .and_then(|t| t.message.clone())
                    })
                })
            })
        })
        .unwrap_or_else(|| "driver pod failed".to_string())
}

/// Enrich an error message with platform context (Celeborn health, etc.).
async fn enrich_error(
    ctx: &Context,
    namespace: &str,
    platform_name: &str,
    base_error: &str,
) -> String {
    let mut enriched = base_error.to_string();

    // Check Celeborn master health.
    let deploy_api: Api<k8s_openapi::api::apps::v1::Deployment> =
        Api::namespaced(ctx.client.clone(), namespace);
    let celeborn_name = format!("{platform_name}-celeborn-master");
    if let Ok(deploy) = deploy_api.get(&celeborn_name).await {
        let ready = deploy
            .status
            .as_ref()
            .and_then(|s| s.ready_replicas)
            .unwrap_or(0);
        if ready == 0 {
            enriched.push_str(
                " [PLATFORM] Celeborn master has 0 ready replicas — shuffle service unavailable",
            );
        }
    }

    // Check Volcano scheduler health.
    let volcano_name = format!("{platform_name}-volcano-scheduler");
    if let Ok(deploy) = deploy_api.get(&volcano_name).await {
        let ready = deploy
            .status
            .as_ref()
            .and_then(|s| s.ready_replicas)
            .unwrap_or(0);
        if ready == 0 {
            enriched.push_str(
                " [PLATFORM] Volcano scheduler has 0 ready replicas — job scheduling may fail",
            );
        }
    }

    enriched
}

/// Log the intent to create a Volcano PodGroup for gang scheduling.
/// Actual PodGroup CRD creation requires Volcano to be installed; this logs the
/// spec so we can verify correctness before wiring up the real apply.
fn log_podgroup_intent(job_name: &str, tenant: &str, num_executors: u32) {
    let queue = tenant; // Volcano queue maps 1:1 to tenant name.
    let min_member = num_executors + 1; // executors + driver
    tracing::info!(
        job_name,
        queue,
        min_member,
        "would create Volcano PodGroup for gang scheduling"
    );
}

/// Handle deletion of a CrucibleSparkJob: kill driver and executor pods.
pub async fn handle_delete(
    job: &CrucibleSparkJob,
    ctx: &Context,
) -> Result<()> {
    let name = job.metadata.name.as_deref().unwrap_or("unknown");
    let namespace = job.metadata.namespace.as_deref().unwrap_or("default");

    let pod_api: Api<Pod> = Api::namespaced(ctx.client.clone(), namespace);

    // Delete driver pod.
    if let Some(ref driver) = job.status.as_ref().and_then(|s| s.driver_pod.clone()) {
        let _ = pod_api.delete(driver, &DeleteParams::default()).await;
    }

    // Delete executor pods.
    let lp = ListParams::default()
        .labels(&format!("crucible.dev/job={name},crucible.dev/role=executor"));
    let _ = pod_api.delete_collection(&DeleteParams::default(), &lp).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crucible_types::spark::{CrucibleSparkJobSpec, ResourceSpec};

    fn test_owner_ref() -> OwnerReference {
        OwnerReference {
            api_version: "crucible.dev/v1".to_string(),
            kind: "CrucibleSparkJob".to_string(),
            name: "test-job".to_string(),
            uid: "test-uid-1234".to_string(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }
    }

    fn sample_spec() -> CrucibleSparkJobSpec {
        CrucibleSparkJobSpec {
            jar: "s3://bucket/app.jar".to_string(),
            class: "com.example.SparkPi".to_string(),
            args: vec!["100".to_string()],
            tenant: "team-a".to_string(),
            executors: Some(4),
            driver_resources: Some(ResourceSpec {
                cpu: Some("2".to_string()),
                memory: Some("4Gi".to_string()),
            }),
            executor_resources: Some(ResourceSpec {
                cpu: Some("4".to_string()),
                memory: Some("8Gi".to_string()),
            }),
            spark_config: BTreeMap::from([(
                "spark.sql.adaptive.enabled".to_string(),
                "true".to_string(),
            )]),
            labels: Default::default(),
        }
    }

    #[test]
    fn spark_config_includes_celeborn_settings() {
        let spec = sample_spec();
        let config = build_spark_config(&spec, "my-platform", "default", "test-job");

        assert_eq!(
            config.get("spark.shuffle.manager").unwrap(),
            "org.apache.celeborn.spark.CelebornShuffleManager"
        );
        assert_eq!(
            config.get("spark.celeborn.master.endpoints").unwrap(),
            "my-platform-celeborn-master.default.svc:9097"
        );
    }

    #[test]
    fn spark_config_includes_event_log() {
        let spec = sample_spec();
        let config = build_spark_config(&spec, "platform", "ns", "test-job");

        assert_eq!(config.get("spark.eventLog.enabled").unwrap(), "true");
        assert!(config.get("spark.eventLog.dir").unwrap().contains("spark-events"));
    }

    #[test]
    fn spark_config_includes_decommissioning() {
        let spec = sample_spec();
        let config = build_spark_config(&spec, "p", "ns", "test-job");

        assert_eq!(config.get("spark.decommission.enabled").unwrap(), "true");
    }

    #[test]
    fn user_config_overrides_defaults() {
        let mut spec = sample_spec();
        spec.spark_config.insert(
            "spark.eventLog.enabled".to_string(),
            "false".to_string(),
        );
        let config = build_spark_config(&spec, "p", "ns", "test-job");

        assert_eq!(config.get("spark.eventLog.enabled").unwrap(), "false");
    }

    #[test]
    fn driver_pod_has_correct_labels() {
        let labels = job_labels("test-job", "driver");
        assert_eq!(labels.get("crucible.dev/job").unwrap(), "test-job");
        assert_eq!(labels.get("crucible.dev/role").unwrap(), "driver");
        assert_eq!(
            labels.get("app.kubernetes.io/managed-by").unwrap(),
            "crucible-operator"
        );
    }

    #[test]
    fn driver_pod_spec_contains_jar_and_class() {
        let spec = sample_spec();
        let config = build_spark_config(&spec, "p", "ns", "test-job");
        let pod = build_driver_pod("job1", "default", "job1-driver", &spec, &config, &test_owner_ref());

        let container = &pod.spec.as_ref().unwrap().containers[0];
        let args_str = container.args.as_ref().unwrap().join(" ");
        assert!(args_str.contains("--class com.example.SparkPi"));
        assert!(args_str.contains("s3://bucket/app.jar"));
        assert!(args_str.contains("100")); // user arg
    }

    #[test]
    fn driver_pod_has_executor_config() {
        let spec = sample_spec();
        let config = build_spark_config(&spec, "p", "ns", "test-job");
        let pod = build_driver_pod("job1", "default", "job1-driver", &spec, &config, &test_owner_ref());

        let args_str = pod.spec.as_ref().unwrap().containers[0]
            .args
            .as_ref()
            .unwrap()
            .join(" ");
        assert!(args_str.contains("spark.executor.instances=4"));
        assert!(args_str.contains("spark.executor.cores=4"));
        assert!(args_str.contains("spark.executor.memory=8Gi"));
    }

    #[test]
    fn driver_pod_has_resource_requests() {
        let spec = sample_spec();
        let config = build_spark_config(&spec, "p", "ns", "test-job");
        let pod = build_driver_pod("job1", "default", "job1-driver", &spec, &config, &test_owner_ref());

        let resources = pod.spec.as_ref().unwrap().containers[0]
            .resources
            .as_ref()
            .unwrap();
        let requests = resources.requests.as_ref().unwrap();
        assert_eq!(requests.get("cpu").unwrap().0, "2");
        assert_eq!(requests.get("memory").unwrap().0, "4Gi");
    }

    #[test]
    fn default_resources_when_none_specified() {
        let mut spec = sample_spec();
        spec.driver_resources = None;
        spec.executor_resources = None;
        spec.executors = None;

        let config = build_spark_config(&spec, "p", "ns", "test-job");
        let pod = build_driver_pod("job1", "default", "job1-driver", &spec, &config, &test_owner_ref());

        let resources = pod.spec.as_ref().unwrap().containers[0]
            .resources
            .as_ref()
            .unwrap();
        let requests = resources.requests.as_ref().unwrap();
        assert_eq!(requests.get("cpu").unwrap().0, DEFAULT_DRIVER_CPU);
        assert_eq!(requests.get("memory").unwrap().0, DEFAULT_DRIVER_MEMORY);

        let args_str = pod.spec.as_ref().unwrap().containers[0]
            .args
            .as_ref()
            .unwrap()
            .join(" ");
        assert!(args_str.contains("spark.executor.instances=1"));
    }

    #[test]
    fn driver_pod_restart_policy_is_never() {
        let spec = sample_spec();
        let config = build_spark_config(&spec, "p", "ns", "test-job");
        let pod = build_driver_pod("job1", "default", "job1-driver", &spec, &config, &test_owner_ref());

        assert_eq!(
            pod.spec.as_ref().unwrap().restart_policy.as_deref(),
            Some("Never")
        );
    }
}
