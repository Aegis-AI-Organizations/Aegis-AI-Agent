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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_node_serialization() {
        let node = HostNode {
            hostname: "test-host".to_string(),
            os: "linux".to_string(),
            kernel: "5.15.0".to_string(),
            uptime: 3600,
            total_ram: 16000000000,
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("test-host"));
        assert!(json.contains("uptime"));

        let decoded: HostNode = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.hostname, "test-host");
    }

    #[test]
    fn test_process_node_serialization() {
        let node = ProcessNode {
            pid: 1234,
            name: "test-proc".to_string(),
            user: "root".to_string(),
            args: Some(vec!["--help".to_string()]),
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("test-proc"));
        assert!(json.contains("1234"));

        let decoded: ProcessNode = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.pid, 1234);
    }
}
