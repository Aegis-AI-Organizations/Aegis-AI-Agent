#[cfg(feature = "docker")]
pub mod docker;
#[cfg(feature = "k8s")]
pub mod k8s;

pub use crate::domain::{ContainerNode, HostNode, PodNode, ProcessNode, SystemExtractor};
#[cfg(feature = "docker")]
pub use docker::DockerExtractor;
#[cfg(feature = "k8s")]
pub use k8s::K8sExtractor;
use std::sync::{Arc, Mutex};
use sysinfo::{ProcessesToUpdate, System};

/// SystemExtractor implementation using the `sysinfo` crate.
///
/// Refreshes are performed inside `spawn_blocking` to avoid blocking the async executor.
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
        let sys_arc = self.sys.clone();

        tokio::task::spawn_blocking(move || {
            let mut sys = sys_arc
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock system mutex"))?;
            sys.refresh_memory();

            Ok(HostNode {
                hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
                os: System::long_os_version().unwrap_or_else(|| "unknown".to_string()),
                kernel: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
                uptime: System::uptime(),
                total_ram: sys.total_memory(),
            })
        })
        .await?
    }

    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>> {
        let sys_arc = self.sys.clone();

        tokio::task::spawn_blocking(move || {
            let mut sys = sys_arc
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock system mutex"))?;
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
                    // Security: Arguments are redacted by default to avoid leaking sensitive data
                    args: None,
                })
                .collect();

            Ok(processes)
        })
        .await?
    }

    async fn get_pods(&self) -> anyhow::Result<Vec<PodNode>> {
        // Sysinfo doesn't know about Kubernetes pods
        Ok(vec![])
    }

    async fn get_containers(&self) -> anyhow::Result<Vec<ContainerNode>> {
        // Sysinfo doesn't know about Docker containers
        Ok(vec![])
    }
}
