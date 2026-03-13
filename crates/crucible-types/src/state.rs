use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::CrucibleError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum JobPhase {
    Submitted,
    Running,
    Completed,
    Failed,
    Killed,
    /// Flink-only: job is suspended with a savepoint.
    Suspended,
    /// Flink-only: job was cancelled.
    Cancelled,
}

impl std::fmt::Display for JobPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Submitted => write!(f, "SUBMITTED"),
            Self::Running => write!(f, "RUNNING"),
            Self::Completed => write!(f, "COMPLETED"),
            Self::Failed => write!(f, "FAILED"),
            Self::Killed => write!(f, "KILLED"),
            Self::Suspended => write!(f, "SUSPENDED"),
            Self::Cancelled => write!(f, "CANCELLED"),
        }
    }
}

impl JobPhase {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Killed | Self::Cancelled
        )
    }

    /// Validate whether a transition from the current phase to the next is allowed.
    pub fn validate_transition(&self, next: &JobPhase) -> Result<(), CrucibleError> {
        let valid = match self {
            Self::Submitted => matches!(next, Self::Running | Self::Failed | Self::Killed),
            Self::Running => matches!(
                next,
                Self::Completed | Self::Failed | Self::Killed | Self::Suspended | Self::Cancelled
            ),
            Self::Suspended => matches!(next, Self::Running | Self::Failed | Self::Cancelled),
            // Terminal states allow no transitions.
            Self::Completed | Self::Failed | Self::Killed | Self::Cancelled => false,
        };

        if valid {
            Ok(())
        } else {
            Err(CrucibleError::InvalidTransition {
                from: self.to_string(),
                to: next.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_spark_lifecycle() {
        assert!(
            JobPhase::Submitted
                .validate_transition(&JobPhase::Running)
                .is_ok()
        );
        assert!(
            JobPhase::Running
                .validate_transition(&JobPhase::Completed)
                .is_ok()
        );
    }

    #[test]
    fn valid_kill_from_any_active() {
        assert!(
            JobPhase::Submitted
                .validate_transition(&JobPhase::Killed)
                .is_ok()
        );
        assert!(
            JobPhase::Running
                .validate_transition(&JobPhase::Killed)
                .is_ok()
        );
    }

    #[test]
    fn invalid_transition_from_terminal() {
        assert!(
            JobPhase::Completed
                .validate_transition(&JobPhase::Running)
                .is_err()
        );
        assert!(
            JobPhase::Failed
                .validate_transition(&JobPhase::Submitted)
                .is_err()
        );
        assert!(
            JobPhase::Killed
                .validate_transition(&JobPhase::Running)
                .is_err()
        );
    }

    #[test]
    fn flink_suspend_resume() {
        assert!(
            JobPhase::Running
                .validate_transition(&JobPhase::Suspended)
                .is_ok()
        );
        assert!(
            JobPhase::Suspended
                .validate_transition(&JobPhase::Running)
                .is_ok()
        );
    }

    #[test]
    fn cannot_go_backwards() {
        assert!(
            JobPhase::Running
                .validate_transition(&JobPhase::Submitted)
                .is_err()
        );
    }
}
