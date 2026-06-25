#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error, uniffi::Error)]
pub enum ClientCoreError {
    #[error("at least one endpoint is required")]
    NoEndpoint,
    #[error("endpoint URL is invalid")]
    InvalidEndpoint,
    #[error("thread id is required")]
    EmptyThreadId,
    #[error("prompt is required")]
    EmptyPrompt,
    #[error("notification id is required")]
    EmptyNotificationId,
    #[error("client mutation id is required")]
    EmptyMutationId,
    #[error("session id is required")]
    EmptySessionId,
    #[error("sequence must be non-negative")]
    InvalidSequence,
    #[error("outbox client mutation IDs did not match the expected order")]
    UnexpectedOutboxMutations,
    #[error("client core state lock is poisoned")]
    StateLockPoisoned,
}
