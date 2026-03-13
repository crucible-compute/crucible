use anyhow::Result;
use crucible_types::platform::CruciblePlatform;

use crate::context::Context;

/// Reconcile Spark History Server deployment.
pub async fn reconcile(platform: &CruciblePlatform, ctx: &Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    tracing::info!(name, "reconciling History Server");

    let _client = &ctx.client;
    let _spec = &platform.spec.history_server;

    // TODO(M1.5): Deploy History Server Deployment + Service.

    Ok(())
}
