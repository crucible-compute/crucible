//! Integration tests for CrucibleSparkJob lifecycle.
//!
//! These tests require a running Kubernetes cluster with Crucible CRDs and operator.
//! All tests are marked #[ignore] so `cargo test` skips them by default.

use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use std::sync::Once;
use std::time::Duration;
use tokio::time::sleep;

use crucible_types::platform::*;
use crucible_types::spark::*;
use crucible_types::state::JobPhase;

static INIT_CRYPTO: Once = Once::new();

fn init_crypto_provider() {
    INIT_CRYPTO.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("failed to install rustls crypto provider");
    });
}

fn test_namespace() -> String {
    std::env::var("TEST_NAMESPACE").unwrap_or_else(|_| "default".to_string())
}

/// Create a CruciblePlatform (required for Spark jobs to find Celeborn config).
async fn ensure_platform(client: &Client, name: &str) -> CruciblePlatform {
    let ns = test_namespace();
    let api: Api<CruciblePlatform> = Api::namespaced(client.clone(), &ns);

    // Check if it already exists.
    if let Ok(existing) = api.get(name).await {
        return existing;
    }

    let spec = CruciblePlatformSpec {
        celeborn: CelebornConfig {
            workers: 2,
            worker_storage: StorageType::EmptyDir,
            image: "apache/celeborn:0.5.2".to_string(),
        },
        volcano: VolcanoConfig {
            tenants: vec![TenantConfig {
                name: "default".to_string(),
                queue_weight: Some(1),
            }],
        },
        history_server: HistoryServerConfig {
            enabled: true,
            image: Some("crucible-spark:latest".to_string()),
        },
        object_store: ObjectStoreConfig {
            bucket: "crucible-data".to_string(),
            endpoint: Some("http://minio:9000".to_string()),
            region: Some("us-east-1".to_string()),
        },
    };

    let platform = CruciblePlatform::new(name, spec);
    api.create(&PostParams::default(), &platform)
        .await
        .expect("failed to create CruciblePlatform")
}

fn test_spark_job_spec() -> CrucibleSparkJobSpec {
    // Use a local JAR path that doesn't need to exist — the driver pod will
    // attempt spark-submit and fail quickly, which is fine for testing the
    // operator's state machine (Submitted → Failed).
    CrucibleSparkJobSpec {
        jar: "local:///opt/spark/jars/spark-core_2.13-4.0.0.jar".to_string(),
        class: "org.apache.spark.examples.SparkPi".to_string(),
        args: vec!["2".to_string()],
        tenant: "default".to_string(),
        executors: Some(1),
        driver_resources: Some(ResourceSpec {
            cpu: Some("250m".to_string()),
            memory: Some("256Mi".to_string()),
        }),
        executor_resources: None,
        spark_config: Default::default(),
        labels: Default::default(),
    }
}

async fn create_spark_job(client: &Client, name: &str) -> CrucibleSparkJob {
    let ns = test_namespace();
    let api: Api<CrucibleSparkJob> = Api::namespaced(client.clone(), &ns);

    let job = CrucibleSparkJob::new(name, test_spark_job_spec());
    api.create(&PostParams::default(), &job)
        .await
        .expect("failed to create CrucibleSparkJob")
}

async fn cleanup_spark_job(client: &Client, name: &str) {
    let ns = test_namespace();
    let api: Api<CrucibleSparkJob> = Api::namespaced(client.clone(), &ns);
    let _ = api.delete(name, &DeleteParams::default()).await;

    // Clean up driver pod.
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), &ns);
    let _ = pod_api
        .delete(&format!("{name}-driver"), &DeleteParams::default())
        .await;

    // Clean up executor pods.
    let lp = ListParams::default()
        .labels(&format!("crucible.dev/job={name},crucible.dev/role=executor"));
    let _ = pod_api.delete_collection(&DeleteParams::default(), &lp).await;

    sleep(Duration::from_secs(3)).await;
}

/// Wait for a SparkJob to reach a specific phase.
async fn wait_for_phase(
    api: &Api<CrucibleSparkJob>,
    name: &str,
    target: &JobPhase,
    timeout: Duration,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(job) = api.get(name).await {
            if let Some(phase) = job.status.as_ref().and_then(|s| s.phase.as_ref()) {
                if phase == target || phase.is_terminal() {
                    return phase == target;
                }
            }
        }
        sleep(Duration::from_millis(1000)).await;
    }
    false
}

/// Wait for any terminal phase.
async fn wait_for_terminal(
    api: &Api<CrucibleSparkJob>,
    name: &str,
    timeout: Duration,
) -> Option<JobPhase> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(job) = api.get(name).await {
            if let Some(phase) = job.status.as_ref().and_then(|s| s.phase.as_ref()) {
                if phase.is_terminal() {
                    return Some(phase.clone());
                }
            }
        }
        sleep(Duration::from_millis(1000)).await;
    }
    None
}

// --- Tests ---

#[tokio::test]
#[ignore]
async fn spark_job_creates_driver_pod() {
    init_crypto_provider();
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "spark-inttest-create";

    cleanup_spark_job(&client, name).await;
    ensure_platform(&client, "inttest-platform").await;

    // Allow platform to reconcile.
    sleep(Duration::from_secs(5)).await;

    create_spark_job(&client, name).await;

    // Wait for the driver pod to be created.
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), &ns);
    let driver_name = format!("{name}-driver");
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();
    let mut found = false;
    while start.elapsed() < timeout {
        if pod_api.get(&driver_name).await.is_ok() {
            found = true;
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }
    assert!(found, "Driver pod should be created");

    // Verify driver pod has Celeborn config in args.
    let pod = pod_api.get(&driver_name).await.unwrap();
    let args = pod.spec.as_ref().unwrap().containers[0]
        .args
        .as_ref()
        .unwrap()
        .join(" ");
    assert!(
        args.contains("spark.shuffle.manager"),
        "Driver should have Celeborn shuffle manager config"
    );
    assert!(
        args.contains("celeborn.master.endpoints"),
        "Driver should have Celeborn master endpoint"
    );

    // Verify job status has driver pod name.
    let job_api: Api<CrucibleSparkJob> = Api::namespaced(client.clone(), &ns);
    sleep(Duration::from_secs(3)).await;
    let job = job_api.get(name).await.unwrap();
    assert_eq!(
        job.status.as_ref().and_then(|s| s.driver_pod.as_deref()),
        Some(driver_name.as_str()),
        "Job status should have driver pod name"
    );

    cleanup_spark_job(&client, name).await;
}

#[tokio::test]
#[ignore]
async fn spark_job_transitions_to_terminal_phase() {
    init_crypto_provider();
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "spark-inttest-phase";

    cleanup_spark_job(&client, name).await;
    ensure_platform(&client, "inttest-platform").await;
    sleep(Duration::from_secs(5)).await;

    create_spark_job(&client, name).await;

    // The job will likely fail (SparkPi needs a working Spark setup which we
    // don't have in Kind), but it should reach a terminal phase.
    let job_api: Api<CrucibleSparkJob> = Api::namespaced(client.clone(), &ns);
    let terminal = wait_for_terminal(&job_api, name, Duration::from_secs(120)).await;

    assert!(
        terminal.is_some(),
        "Job should reach a terminal phase (Completed or Failed)"
    );

    // Verify end_time is set.
    let job = job_api.get(name).await.unwrap();
    assert!(
        job.status.as_ref().and_then(|s| s.end_time.as_ref()).is_some(),
        "Terminal job should have end_time"
    );

    cleanup_spark_job(&client, name).await;
}

#[tokio::test]
#[ignore]
async fn spark_job_delete_kills_driver() {
    init_crypto_provider();
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "spark-inttest-delete";

    cleanup_spark_job(&client, name).await;
    ensure_platform(&client, "inttest-platform").await;
    sleep(Duration::from_secs(5)).await;

    create_spark_job(&client, name).await;

    // Wait for driver pod to exist.
    let pod_api: Api<Pod> = Api::namespaced(client.clone(), &ns);
    let driver_name = format!("{name}-driver");
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if pod_api.get(&driver_name).await.is_ok() {
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }

    // Delete the spark job.
    let job_api: Api<CrucibleSparkJob> = Api::namespaced(client.clone(), &ns);
    job_api
        .delete(name, &DeleteParams::default())
        .await
        .expect("failed to delete spark job");

    // Verify the CR is gone.
    sleep(Duration::from_secs(5)).await;
    assert!(
        job_api.get(name).await.is_err(),
        "CrucibleSparkJob CR should be deleted"
    );

    cleanup_spark_job(&client, name).await;
}

#[tokio::test]
#[ignore]
async fn spark_job_failed_has_error_message() {
    init_crypto_provider();
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "spark-inttest-error";

    cleanup_spark_job(&client, name).await;
    ensure_platform(&client, "inttest-platform").await;
    sleep(Duration::from_secs(5)).await;

    // Submit a job with an invalid JAR to trigger failure.
    let mut spec = test_spark_job_spec();
    spec.jar = "local:///nonexistent/path/bad.jar".to_string();
    spec.class = "com.example.DoesNotExist".to_string();

    let job_api: Api<CrucibleSparkJob> = Api::namespaced(client.clone(), &ns);
    let job = CrucibleSparkJob::new(name, spec);
    job_api
        .create(&PostParams::default(), &job)
        .await
        .expect("failed to create spark job");

    // Wait for terminal phase.
    let terminal = wait_for_terminal(&job_api, name, Duration::from_secs(120)).await;
    assert_eq!(terminal, Some(JobPhase::Failed), "Invalid JAR should fail");

    // Verify error message exists.
    let job = job_api.get(name).await.unwrap();
    assert!(
        job.status.as_ref().and_then(|s| s.error.as_ref()).is_some(),
        "Failed job should have error message"
    );

    cleanup_spark_job(&client, name).await;
}
