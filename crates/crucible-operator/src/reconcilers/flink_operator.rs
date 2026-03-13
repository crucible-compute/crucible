use anyhow::Result;
use crucible_types::platform::CruciblePlatform;

use crate::context::Context;

/// Reconcile Flink Kubernetes Operator installation.
pub async fn reconcile(platform: &CruciblePlatform, ctx: &Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    tracing::info!(name, "reconciling Flink Operator");

    let _client = &ctx.client;

    // TODO(M1.6): Install Flink Operator from manifest bundle.

    Ok(())
}
