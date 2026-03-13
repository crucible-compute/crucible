mod context;
mod health;
mod reconcilers;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::Result;
use futures::StreamExt;
use kube::Client;
use kube::api::Api;
use kube::runtime::Controller;
use kube::runtime::watcher::Config;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crucible_types::platform::CruciblePlatform;
use crucible_types::spark::CrucibleSparkJob;

use crate::context::Context;
use crate::reconcilers::{platform, spark_job};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // Install TLS crypto provider before any kube client usage.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    tracing::info!("crucible-operator starting");

    let client = Client::try_default().await?;
    let ctx = Arc::new(Context::new(client.clone()));

    // Health server
    let health_state = Arc::new(health::HealthState::new());
    let health_port = std::env::var("HEALTH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8081u16);

    let health_handle = {
        let state = health_state.clone();
        tokio::spawn(async move {
            if let Err(e) = health::serve(state, health_port).await {
                tracing::error!(%e, "health server failed");
            }
        })
    };

    // TODO(M6.7): Add leader election via Lease before starting controller.

    tracing::info!("starting controllers");
    health_state.ready.store(true, Ordering::Relaxed);

    let platforms: Api<CruciblePlatform> = Api::all(client.clone());
    let spark_jobs: Api<CrucibleSparkJob> = Api::all(client);

    let platform_ctrl = Controller::new(platforms, Config::default())
        .run(platform::reconcile, platform::error_policy, ctx.clone())
        .for_each(|res| async move {
            match res {
                Ok(o) => tracing::debug!(?o, "platform reconciled"),
                Err(e) => tracing::error!(%e, "platform reconcile failed"),
            }
        });

    let spark_ctrl = Controller::new(spark_jobs, Config::default())
        .run(spark_job::reconcile, spark_job::error_policy, ctx)
        .for_each(|res| async move {
            match res {
                Ok(o) => tracing::debug!(?o, "spark job reconciled"),
                Err(e) => tracing::error!(%e, "spark job reconcile failed"),
            }
        });

    // Run both controllers concurrently. If either exits, the operator stops.
    tokio::select! {
        _ = platform_ctrl => tracing::warn!("platform controller exited"),
        _ = spark_ctrl => tracing::warn!("spark job controller exited"),
    }

    health_handle.abort();
    Ok(())
}

fn init_tracing() {
    let is_production = std::env::var("CRUCIBLE_ENV")
        .unwrap_or_default()
        .eq_ignore_ascii_case("production");

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if is_production {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().pretty())
            .init();
    }
}
