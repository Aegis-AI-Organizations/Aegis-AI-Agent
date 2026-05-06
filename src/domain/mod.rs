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
