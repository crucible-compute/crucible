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

    tracing::info!("crucible-operator starting");

    // Operator controller setup will be added in M1.
    tracing::info!("crucible-operator ready (no controllers registered yet)");

    Ok(())
}
