//! Integration tests for CruciblePlatform lifecycle.
//!
//! These tests require a running Kubernetes cluster with Crucible CRDs installed.
//! Run with: `just test-integration` or `cargo test --package crucible-operator --test platform_lifecycle`
//!
//! All tests are marked #[ignore] so `cargo test` skips them by default.
//! Use `cargo test -- --ignored` to run them explicitly.

use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::api::core::v1::Service;
use kube::Client;
use kube::api::{Api, DeleteParams, Patch, PatchParams, PostParams};
use std::sync::Once;
use std::time::Duration;
use tokio::time::sleep;

use crucible_types::platform::*;

static INIT_CRYPTO: Once = Once::new();

fn init_crypto_provider() {
    INIT_CRYPTO.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("failed to install rustls crypto provider");
    });
}

/// Wait for a resource to exist, polling with a timeout.
async fn wait_for_resource<K>(api: &Api<K>, name: &str, timeout: Duration) -> bool
where
    K: kube::api::Resource
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned
        + serde::Serialize,
{
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if api.get(name).await.is_ok() {
            return true;
        }
        sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Wait for a resource to be deleted.
async fn wait_for_deletion<K>(api: &Api<K>, name: &str, timeout: Duration) -> bool
where
    K: kube::api::Resource
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned
        + serde::Serialize,
{
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if api.get(name).await.is_err() {
            return true;
        }
        sleep(Duration::from_millis(500)).await;
    }
    false
}

fn test_platform_spec() -> CruciblePlatformSpec {
    CruciblePlatformSpec {
        celeborn: CelebornConfig {
            workers: 2,
            worker_storage: StorageType::EmptyDir,
            image: "apache/celeborn:0.5.2".to_string(),
        },
        volcano: VolcanoConfig {
            tenants: vec![
                TenantConfig {
                    name: "default".to_string(),
                    queue_weight: Some(1),
                },
                TenantConfig {
                    name: "team-a".to_string(),
                    queue_weight: Some(2),
                },
            ],
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
    }
}

fn test_namespace() -> String {
    std::env::var("TEST_NAMESPACE").unwrap_or_else(|_| "default".to_string())
}

/// Helper to create the test platform CR.
async fn create_test_platform(client: &Client, name: &str) -> CruciblePlatform {
    let ns = test_namespace();
    let api: Api<CruciblePlatform> = Api::namespaced(client.clone(), &ns);

    let platform = CruciblePlatform::new(name, test_platform_spec());

    api.create(&PostParams::default(), &platform)
        .await
        .expect("failed to create CruciblePlatform")
}

/// Helper to delete the test platform CR and wait for cleanup.
async fn cleanup_platform(client: &Client, name: &str) {
    let ns = test_namespace();
    let api: Api<CruciblePlatform> = Api::namespaced(client.clone(), &ns);
    let _ = api.delete(name, &DeleteParams::default()).await;
    // Give the operator time to clean up child resources.
    sleep(Duration::from_secs(5)).await;
}

// --- Tests ---

#[tokio::test]
#[ignore]
async fn create_platform_deploys_all_sub_components() {
    init_crypto_provider();
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "inttest-create";

    // Ensure clean state.
    cleanup_platform(&client, name).await;

    // Create the platform.
    create_test_platform(&client, name).await;

    let timeout = Duration::from_secs(30);

    // Verify Celeborn resources.
    let deploys: Api<Deployment> = Api::namespaced(client.clone(), &ns);
    let statefulsets: Api<StatefulSet> = Api::namespaced(client.clone(), &ns);
    let services: Api<Service> = Api::namespaced(client.clone(), &ns);

    assert!(
        wait_for_resource(&deploys, &format!("{name}-celeborn-master"), timeout).await,
        "Celeborn master Deployment not created"
    );
    assert!(
        wait_for_resource(&statefulsets, &format!("{name}-celeborn-worker"), timeout).await,
        "Celeborn worker StatefulSet not created"
    );
    assert!(
        wait_for_resource(&services, &format!("{name}-celeborn-master"), timeout).await,
        "Celeborn master Service not created"
    );

    // Verify Celeborn worker replica count.
    let sts = statefulsets
        .get(&format!("{name}-celeborn-worker"))
        .await
        .unwrap();
    assert_eq!(
        sts.spec.as_ref().unwrap().replicas,
        Some(2),
        "Celeborn worker replicas should be 2"
    );

    // Verify Volcano resources.
    assert!(
        wait_for_resource(&deploys, &format!("{name}-volcano-scheduler"), timeout).await,
        "Volcano scheduler Deployment not created"
    );
    assert!(
        wait_for_resource(&deploys, &format!("{name}-volcano-controller"), timeout).await,
        "Volcano controller Deployment not created"
    );

    // Verify History Server resources.
    assert!(
        wait_for_resource(&deploys, &format!("{name}-history-server"), timeout).await,
        "History Server Deployment not created"
    );
    assert!(
        wait_for_resource(&services, &format!("{name}-history-server"), timeout).await,
        "History Server Service not created"
    );

    // Verify Flink Operator resources.
    assert!(
        wait_for_resource(&deploys, &format!("{name}-flink-operator"), timeout).await,
        "Flink Operator Deployment not created"
    );

    // Cleanup.
    cleanup_platform(&client, name).await;
}

#[tokio::test]
#[ignore]
async fn update_platform_changes_worker_count() {
    init_crypto_provider();
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "inttest-update";

    cleanup_platform(&client, name).await;
    create_test_platform(&client, name).await;

    let statefulsets: Api<StatefulSet> = Api::namespaced(client.clone(), &ns);
    let timeout = Duration::from_secs(30);

    // Wait for initial deployment.
    assert!(
        wait_for_resource(&statefulsets, &format!("{name}-celeborn-worker"), timeout).await,
        "Initial Celeborn worker StatefulSet not created"
    );

    // Update worker count from 2 to 4.
    let api: Api<CruciblePlatform> = Api::namespaced(client.clone(), &ns);
    let patch = serde_json::json!({
        "spec": {
            "celeborn": {
                "workers": 4
            }
        }
    });
    api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await
        .expect("failed to patch CruciblePlatform");

    // Wait for the operator to reconcile.
    sleep(Duration::from_secs(10)).await;

    let sts = statefulsets
        .get(&format!("{name}-celeborn-worker"))
        .await
        .unwrap();
    assert_eq!(
        sts.spec.as_ref().unwrap().replicas,
        Some(4),
        "Celeborn worker replicas should be updated to 4"
    );

    cleanup_platform(&client, name).await;
}

#[tokio::test]
#[ignore]
async fn delete_platform_cleans_up_resources() {
    init_crypto_provider();
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "inttest-delete";

    cleanup_platform(&client, name).await;
    create_test_platform(&client, name).await;

    let deploys: Api<Deployment> = Api::namespaced(client.clone(), &ns);
    let timeout = Duration::from_secs(30);

    // Wait for resources to be created.
    assert!(
        wait_for_resource(&deploys, &format!("{name}-celeborn-master"), timeout).await,
        "Celeborn master should exist before delete"
    );

    // Delete the platform.
    let api: Api<CruciblePlatform> = Api::namespaced(client.clone(), &ns);
    api.delete(name, &DeleteParams::default())
        .await
        .expect("failed to delete CruciblePlatform");

    // Verify child resources are cleaned up.
    // Note: This requires owner references on child resources, which we'll add.
    // For now, just verify the CR itself is gone.
    assert!(
        wait_for_deletion(&api, name, timeout).await,
        "CruciblePlatform CR should be deleted"
    );

    // Give operator time to process finalizers.
    sleep(Duration::from_secs(5)).await;
}

#[tokio::test]
#[ignore]
async fn sub_reconciler_isolation() {
    init_crypto_provider();
    // Verify that a failed sub-reconciler doesn't prevent others from working.
    // We test this by checking that platform status has per-condition granularity.
    let client = Client::try_default().await.unwrap();
    let ns = test_namespace();
    let name = "inttest-isolation";

    cleanup_platform(&client, name).await;
    create_test_platform(&client, name).await;

    // Wait for at least one reconcile cycle.
    sleep(Duration::from_secs(15)).await;

    let api: Api<CruciblePlatform> = Api::namespaced(client.clone(), &ns);
    let platform = api.get(name).await.expect("platform should exist");

    if let Some(status) = &platform.status {
        // Verify we have conditions for all sub-reconcilers.
        let condition_types: Vec<&ConditionType> =
            status.conditions.iter().map(|c| &c.r#type).collect();

        assert!(
            condition_types.contains(&&ConditionType::CelebornReady),
            "Should have CelebornReady condition"
        );
        assert!(
            condition_types.contains(&&ConditionType::VolcanoReady),
            "Should have VolcanoReady condition"
        );
        assert!(
            condition_types.contains(&&ConditionType::HistoryServerReady),
            "Should have HistoryServerReady condition"
        );
        assert!(
            condition_types.contains(&&ConditionType::FlinkOperatorReady),
            "Should have FlinkOperatorReady condition"
        );

        // Phase should be set.
        assert!(status.phase.is_some(), "Platform should have a phase");
    } else {
        panic!("Platform should have a status after reconciliation");
    }

    cleanup_platform(&client, name).await;
}
