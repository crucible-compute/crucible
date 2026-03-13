use kube::Client;

/// Shared state available to all reconcilers.
#[derive(Clone)]
pub struct Context {
    pub client: Client,
}

impl Context {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}
