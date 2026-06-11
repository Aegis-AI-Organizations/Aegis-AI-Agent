#[cfg(feature = "docker")]
pub mod docker;
#[cfg(feature = "k8s")]
pub mod k8s;

pub use crate::domain::{
    ActiveResource, ContainerNode, HostNode, PodNode, ProcessNode, SystemExtractor,
    TopologyExtractor,
};
use crate::redaction::Redactor;
#[cfg(feature = "docker")]
pub use docker::DockerExtractor;
#[cfg(feature = "k8s")]
pub use k8s::K8sExtractor;
use std::sync::OnceLock;
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

static ENV_REDACTOR: OnceLock<Redactor> = OnceLock::new();

pub fn redact_environment_entry(key: &str, value: Option<&str>) -> String {
    let key_upper = key.to_ascii_uppercase();

    if let Some(mock_value) = mock_sensitive_environment_value(&key_upper) {
        return mock_value.to_string();
    }

    let Some(value) = value else {
        return "aegis-mock-value".to_string();
    };

    materialize_redacted_environment_value(
        &ENV_REDACTOR.get_or_init(Redactor::new).redact(value),
        value,
    )
}

pub fn redact_environment_value(value: &str) -> String {
    materialize_redacted_environment_value(
        &ENV_REDACTOR.get_or_init(Redactor::new).redact(value),
        value,
    )
}

pub fn redact_environment_value_with_redactor(
    key: &str,
    value: &str,
    redactor: &Redactor,
) -> String {
    let key_upper = key.to_ascii_uppercase();

    if let Some(mock_value) = mock_sensitive_environment_value(&key_upper) {
        return mock_value.to_string();
    }

    materialize_redacted_environment_value(&redactor.redact(value), value)
}

fn mock_sensitive_environment_value(key_upper: &str) -> Option<&'static str> {
    if key_upper.contains("AWS_ACCESS_KEY_ID") || key_upper == "AWS_KEY" {
        return Some("AKIA0000000000000000");
    }

    if key_upper.contains("AWS_SECRET_ACCESS_KEY") {
        return Some("aegis-mock-aws-secret");
    }

    if key_upper.contains("PASSWORD")
        || key_upper.contains("PASS")
        || key_upper.contains("PWD")
        || key_upper.contains("SECRET")
        || key_upper.contains("PRIVATE_KEY")
    {
        return Some("aegis-mock-secret");
    }

    if key_upper.contains("API_KEY") || key_upper.ends_with("_KEY") {
        return Some("aegis-mock-api-key");
    }

    if key_upper.contains("TOKEN") {
        return Some("aegis-mock-token");
    }

    None
}

fn materialize_redacted_environment_value(redacted: &str, original: &str) -> String {
    if redacted == original {
        return original.to_string();
    }

    redacted
        .replace("<REDACTED_AWS_KEY>", "AKIA0000000000000000")
        .replace("<REDACTED_SECRET>", "aegis-mock-secret")
        .replace("<REDACTED_PERSON>", "PRENOM_1 NOM_1")
        .replace("<REDACTED_ORG>", "ORGANISATION_1")
        .replace("<REDACTED_LOC>", "VILLE_1")
        .replace("<REDACTED_IP>", "203.0.113.10")
}

pub fn parse_port_key(value: &str) -> Option<(i32, String)> {
    let (port, protocol) = value.split_once('/').unwrap_or((value, "tcp"));
    let number = port.parse::<i32>().ok()?;
    Some((number, protocol.to_lowercase()))
}

pub fn looks_sensitive_volume(path: &str) -> bool {
    let lower = path.to_lowercase();
    [
        "docker.sock",
        "/var/run/secrets",
        "/run/secrets",
        "/etc",
        "/root",
        "/home/",
        ".kube",
        ".aws",
        "id_rsa",
        "id_ed25519",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

pub fn is_root_user(user: Option<&str>) -> Option<bool> {
    user.map(|value| {
        let normalized = value.trim().to_lowercase();
        normalized == "0" || normalized == "root" || normalized.starts_with("0:")
    })
}

pub fn should_include_runtime_container(name: &str, image: &str) -> bool {
    if read_bool_env("TOPOLOGY_INCLUDE_ALL_RUNTIME_CONTAINERS") {
        return true;
    }

    let normalized_name = name.to_lowercase();
    let normalized_image = image.to_lowercase();

    if let Some(allowlist) = read_csv_env("TOPOLOGY_CONTAINER_ALLOWLIST") {
        let allowed = allowlist
            .iter()
            .map(|value| value.to_lowercase())
            .any(|token| normalized_name.contains(&token) || normalized_image.contains(&token));
        return allowed;
    }

    if normalized_name.starts_with("k8s_pod_") {
        return false;
    }

    if (normalized_name.starts_with("k8s_") && normalized_name.contains("_kube-system_"))
        || normalized_name.contains("_cert-manager_")
        || normalized_name.contains("_cilium_")
        || normalized_name.contains("_local-path-provisioner_")
        || normalized_name.contains("_ingress-nginx_")
        || normalized_name.contains("_traefik_")
    {
        return false;
    }

    if normalized_image.contains("pause") || normalized_image.contains("sandbox") {
        return false;
    }

    if normalized_name.contains("cert-manager")
        || normalized_name.contains("cilium")
        || normalized_name.contains("local-path-provisioner")
    {
        return false;
    }

    true
}

pub fn should_include_k8s_namespace(namespace: &str) -> bool {
    if read_bool_env("TOPOLOGY_INCLUDE_SYSTEM_K8S") {
        return true;
    }

    let namespace = namespace.trim();
    if namespace.is_empty() {
        return true;
    }

    if let Some(allowlist) = read_csv_env("TOPOLOGY_K8S_NAMESPACE_ALLOWLIST") {
        return allowlist.iter().any(|value| value == namespace);
    }

    !matches!(
        namespace,
        "kube-system"
            | "kube-public"
            | "kube-node-lease"
            | "cert-manager"
            | "cilium"
            | "istio-system"
            | "linkerd"
            | "local-path-provisioner"
            | "ingress-nginx"
            | "traefik"
            | "longhorn-system"
            | "rook-ceph"
            | "monitoring"
            | "metrics"
            | "kyverno"
            | "gatekeeper-system"
            | "argocd"
            | "velero"
    ) && !namespace.starts_with("kube-")
        && !namespace.starts_with("cilium")
        && !namespace.starts_with("cert-manager")
}

fn read_bool_env(primary: &str) -> bool {
    read_env_value(primary)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn read_csv_env(primary: &str) -> Option<Vec<String>> {
    read_env_value(primary)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
}

fn read_env_value(primary: &str) -> Option<String> {
    std::env::var(primary).ok()
}
