use serde::{Deserialize, Serialize};

/// Représente l'infrastructure hôte du client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostNode {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub uptime: u64,
    pub total_ram: u64,
}

/// Représente un processus en cours d'exécution sur le système.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessNode {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub args: Option<Vec<String>>,
}

/// Contrat d'extraction des données système.
pub trait SystemExtractor: Send + Sync {
    async fn get_host_info(&self) -> anyhow::Result<HostNode>;
    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>>;
}
