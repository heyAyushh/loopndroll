use tonic::transport::Endpoint;

use crate::ClientCoreError;

pub(crate) fn validate_endpoint_url(url: &str) -> Result<(), ClientCoreError> {
    if url.trim().is_empty() {
        return Err(ClientCoreError::InvalidEndpoint);
    }

    Endpoint::from_shared(url.to_owned())
        .map(|_| ())
        .map_err(|_| ClientCoreError::InvalidEndpoint)
}
