use axum::{Router, routing::get};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::TcpListener;

/// Shared health state — set to true once controllers are running.
pub struct HealthState {
    pub ready: AtomicBool,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
        }
    }
}

/// Start the health server on the given port.
pub async fn serve(state: Arc<HealthState>, port: u16) -> anyhow::Result<()> {
    let app = Router::new().route("/healthz", get(healthz)).route(
        "/readyz",
        get({
            let state = state.clone();
            move || readyz(state.clone())
        }),
    );

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("health server listening on {addr}");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(state: Arc<HealthState>) -> (axum::http::StatusCode, &'static str) {
    if state.ready.load(Ordering::Relaxed) {
        (axum::http::StatusCode::OK, "ready")
    } else {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, "not ready")
    }
}
