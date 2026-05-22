#[cfg(feature = "docker")]
pub mod docker;
#[cfg(feature = "k8s")]
pub mod k8s;

pub use crate::domain::{
    ActiveResource, ContainerNode, HostNode, PodNode, ProcessNode, SystemExtractor,
    TopologyExtractor,
};
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
                .collect::<Vec<_>>();

            Ok(filter_host_processes(processes))
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

pub fn filter_host_processes(mut processes: Vec<ProcessNode>) -> Vec<ProcessNode> {
    let current_pid = std::process::id();
    let include_all = std::env::var("AEGIS_INCLUDE_ALL_HOST_PROCESSES")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    let max_processes = std::env::var("AEGIS_MAX_HOST_PROCESSES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);

    processes.retain(|process| {
        process.pid == current_pid
            || (!process.name.is_empty()
                && process.user != "unknown"
                && (include_all || is_relevant_host_process(process)))
    });
    processes.sort_by_key(|left| left.pid);

    if processes.len() > max_processes {
        let mut tail = processes.split_off(processes.len() - max_processes);
        if !tail.iter().any(|process| process.pid == current_pid) {
            if let Some(current_process) = processes.into_iter().find(|p| p.pid == current_pid) {
                tail.remove(0);
                tail.push(current_process);
                tail.sort_by_key(|left| left.pid);
            }
        }
        tail
    } else {
        processes
    }
}

fn is_relevant_host_process(process: &ProcessNode) -> bool {
    let name = process.name.to_lowercase();

    if is_explicitly_ignored_host_process(&name) {
        return false;
    }

    const RELEVANT_EXACT_NAMES: &[&str] = &[
        "aegis-ai-agent",
        "docker",
        "docker-compose",
        "docker desktop",
        "docker desktop helper",
        "docker desktop helper (gpu)",
        "orbstack",
        "orbstack helper",
        "cloudflared",
        "nginx",
        "redis-server",
        "ollama",
        "node",
        "python",
        "python3",
        "ruby",
        "java",
        "cargo",
        "rustc",
        "npm",
        "pnpm",
        "yarn",
        "bun",
        "deno",
        "vite",
        "next",
        "uvicorn",
        "gunicorn",
        "zsh",
        "bash",
        "fish",
        "tmux",
        "ssh-agent",
        "kubectl",
        "helm",
        "minikube",
        "kind",
        "k3d",
        "colima",
        "podman",
    ];
    const RELEVANT_SUBSTRINGS: &[&str] = &[
        "aegis-",
        "aegis_",
        "com.docker.",
        "containerd",
        "dockerd",
        "kube",
        "kub",
        "minio",
        "postgres",
        "postgresql",
        "redis",
        "temporal",
        "gateway",
        "cloudflared",
        "nginx",
        "brain",
    ];

    RELEVANT_EXACT_NAMES.contains(&name.as_str())
        || RELEVANT_SUBSTRINGS
            .iter()
            .any(|pattern| name.contains(pattern))
}

fn is_explicitly_ignored_host_process(name: &str) -> bool {
    const IGNORED_PATTERNS: &[&str] = &[
        "browser helper",
        "crashpad",
        "webkit",
        "widget",
        "pluginlibraryservice",
        "extensionkitservice",
        "mdworker",
        "mtlcompilerservice",
        "vtdecoderxpcservice",
        "vtencoderxpcservice",
        "cfprefsd",
        "distnoted",
        "quicklook",
        "coresymbolicationd",
        "wallpaper",
        "photos",
        "safari",
        "themewidget",
        "messagesblastdoorservice",
        "com.apple.",
        "cloudtelemetryservice",
        "speechsynthesisserverxpc",
        "extension",
        "widget",
        "helper (renderer)",
        "helper (gpu)",
        "service",
        "agent",
    ];

    IGNORED_PATTERNS
        .iter()
        .any(|pattern| name.contains(pattern))
        && !matches!(
            name,
            "aegis-ai-agent"
                | "ssh-agent"
                | "orbstack helper"
                | "docker desktop helper"
                | "docker desktop helper (gpu)"
        )
}
