use anyhow::Result;
use crucible_types::platform::CruciblePlatform;

use crate::context::Context;

/// Reconcile Celeborn master + worker StatefulSet.
pub async fn reconcile(platform: &CruciblePlatform, ctx: &Context) -> Result<()> {
    let name = platform.metadata.name.as_deref().unwrap_or("unknown");
    tracing::info!(name, "reconciling Celeborn");

    let _client = &ctx.client;
    let _spec = &platform.spec.celeborn;

    // TODO(M1.3): Deploy Celeborn master Deployment and worker StatefulSet.

    Ok(())
}
