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

use crate::context::Context;
use crate::reconcilers::platform;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

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

    // TODO(M1.2a): Add leader election via Lease before starting controller.

    tracing::info!("starting CruciblePlatform controller");
    health_state.ready.store(true, Ordering::Relaxed);

    let platforms: Api<CruciblePlatform> = Api::all(client);

    Controller::new(platforms, Config::default())
        .run(platform::reconcile, platform::error_policy, ctx)
        .for_each(|res| async move {
            match res {
                Ok(o) => tracing::debug!(?o, "reconciled"),
                Err(e) => tracing::error!(%e, "reconcile failed"),
            }
        })
        .await;

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
