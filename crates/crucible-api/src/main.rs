use axum::Router;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    let app = Router::new().route("/healthz", axum::routing::get(|| async { "ok" }));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("crucible-api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
