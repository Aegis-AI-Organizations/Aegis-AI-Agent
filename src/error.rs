use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Access denied: invalid or missing authentication token")]
    AccessDenied,

    #[error("Network connection failed: {0}")]
    NetworkFailed(String),

    #[error("Missing configuration: {0}")]
    MissingConfig(String),

    #[error("Access denied to Docker socket: check permissions or group membership")]
    DockerPermissionDenied,

    #[error("Access denied to Kubernetes API (RBAC): check service account permissions")]
    K8sPermissionDenied,

    #[error("Internal agent error: {0}")]
    Internal(String),
}
