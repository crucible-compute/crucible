use thiserror::Error;

#[derive(Debug, Error)]
pub enum CrucibleError {
    #[error("invalid state transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },

    #[error("validation error: {0}")]
    Validation(String),

    #[error("kubernetes error: {0}")]
    Kube(#[from] kube::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
