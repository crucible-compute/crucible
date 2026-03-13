use anyhow::Result;
use crucible_types::platform::CruciblePlatform;

use crate::context::Context;

/// Reconcile Volcano installation and tenant queues.
pub async fn reconcile(platform: &CruciblePlatform, ctx: &Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    tracing::info!(name, "reconciling Volcano");

    let _client = &ctx.client;
    let _spec = &platform.spec.volcano;

    // TODO(M1.4): Install Volcano CRDs/controller and create tenant queues.

    Ok(())
}
