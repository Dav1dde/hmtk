mod device;

pub use self::device::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Expected a valid device status.
    #[error("expected valid device status, got: {0:?}")]
    InvalidStatus(#[from] InvalidStatus),
    #[error("failed to publish mqttt message {0}")]
    MqttClientError(#[from] rumqttc::ClientError),
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidStatus {
    #[error("expected valid device status, got: {0:?}")]
    InvalidFormat(bytes::Bytes),
    #[error("field '{0}' contains invalid data: {1}")]
    InvalidField(
        &'static str,
        #[source] Box<dyn std::error::Error + Send + Sync>,
    ),
    #[error("field '{0}' is required, but missing in the status message")]
    MissingField(&'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
