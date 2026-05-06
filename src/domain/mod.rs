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
