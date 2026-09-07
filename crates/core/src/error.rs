use thiserror::Error;

/// Errors from the pure core.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The flow JSON uses a version this build does not implement.
    #[error("unsupported flow version {0} (this build supports version 1)")]
    UnsupportedFlowVersion(u32),

    /// A flow references a condition-field key that is not in the registry.
    #[error("unknown condition field `{0}`")]
    UnknownConditionField(String),

    /// A flow references an operation-section key that is not in the registry.
    #[error("unknown operation section `{0}`")]
    UnknownOperationSection(String),

    /// A flow (or one of its fields/sections) failed to parse.
    #[error("flow error: {0}")]
    Flow(String),

    /// The file facts are malformed for a computation.
    #[error("facts error: {0}")]
    Facts(String),

    /// A verification check found the output does not match the plan.
    #[error("verification failed: {0}")]
    Verification(String),

    /// A planned stream cannot fit the explicitly chosen container and
    /// the MKV fallback is disabled (DESIGN §6.4) — the file fails at
    /// plan time, before any encode is attempted.
    #[error(
        "stream(s) do not fit container '{target}' and the MKV fallback is off — enable the fallback or choose a container that fits"
    )]
    ContainerIncompatible { target: &'static str },
}

/// Core result alias.
pub type Result<T> = std::result::Result<T, CoreError>;
