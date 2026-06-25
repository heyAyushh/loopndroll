#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error, uniffi::Error)]
pub enum ClientCoreError {
    #[error("at least one endpoint is required")]
    NoEndpoint,
    #[error("endpoint URL is invalid")]
    InvalidEndpoint,
    #[error("thread id is required")]
    EmptyThreadId,
    #[error("mode preset is required")]
    EmptyPreset,
    #[error("prompt is required")]
    EmptyPrompt,
    #[error("notification id is required")]
    EmptyNotificationId,
    #[error("client mutation id is required")]
    EmptyMutationId,
    #[error("client core state lock is poisoned")]
    StateLockPoisoned,
}
