use serde::{Deserialize, Serialize};

/// Represents the host infrastructure of the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostNode {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    /// System uptime in seconds.
    pub uptime: u64,
    /// Total RAM available in bytes.
    pub total_ram: u64,
}

/// Represents a running process on the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessNode {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub args: Option<Vec<String>>,
}

/// Interface for system data extraction.
#[allow(async_fn_in_trait)]
pub trait SystemExtractor: Send + Sync {
    /// Retrieves information about the host system.
    async fn get_host_info(&self) -> anyhow::Result<HostNode>;
    /// Retrieves a list of currently running processes.
    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>>;
}
