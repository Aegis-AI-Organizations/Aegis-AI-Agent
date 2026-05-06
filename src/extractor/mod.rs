use crate::domain::{HostNode, ProcessNode, SystemExtractor};
use std::sync::Arc;
use sysinfo::{ProcessesToUpdate, System};
use tokio::sync::Mutex;

/// Implémentation du SystemExtractor utilisant la crate `sysinfo`.
pub struct SysinfoExtractor {
    sys: Arc<Mutex<System>>,
}

impl SysinfoExtractor {
    pub fn new() -> Self {
        Self {
            sys: Arc::new(Mutex::new(System::new_all())),
        }
    }
}

impl Default for SysinfoExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemExtractor for SysinfoExtractor {
    async fn get_host_info(&self) -> anyhow::Result<HostNode> {
        let mut sys = self.sys.lock().await;
        sys.refresh_all();

        Ok(HostNode {
            hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
            os: System::long_os_version().unwrap_or_else(|| "unknown".to_string()),
            kernel: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
            uptime: System::uptime(),
            total_ram: sys.total_memory(),
        })
    }

    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>> {
        let mut sys = self.sys.lock().await;
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let processes = sys
            .processes()
            .values()
            .map(|p| ProcessNode {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().to_string(),
                user: p
                    .user_id()
                    .map(|u| format!("{:?}", u))
                    .unwrap_or_else(|| "unknown".to_string()),
                args: Some(
                    p.cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_string())
                        .collect(),
                ),
            })
            .collect();

        Ok(processes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sysinfo_extractor_real_data() {
        let extractor = SysinfoExtractor::new();

        // Test Host Info
        let host_info = extractor
            .get_host_info()
            .await
            .expect("Failed to get host info");
        println!("Host Info: {:?}", host_info);
        assert!(
            !host_info.hostname.is_empty(),
            "Hostname should not be empty"
        );
        assert!(
            host_info.total_ram > 0,
            "Total RAM should be greater than 0"
        );

        // Test Processes
        let processes = extractor
            .get_processes()
            .await
            .expect("Failed to get processes");
        println!("Found {} processes", processes.len());
        assert!(!processes.is_empty(), "Process list should not be empty");

        // Vérifie que le processus actuel est présent
        let current_pid = std::process::id();
        let found = processes.iter().any(|p| p.pid == current_pid);
        assert!(
            found,
            "Current PID {} not found in process list",
            current_pid
        );
    }
}
