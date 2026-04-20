# 🦀 Aegis AI — Infrastructure Agent

**Project ID:** AEGIS-CORE-2026

> The **Aegis AI Agent** is the high-performance collection sensor deployed within target environments. Written in **Rust**, it captures real-time telemetry and securely streams it to the Aegis Ingest workers via **mTLS-encrypted gRPC** channels.

---

## 🏗️ Role in the Ecosystem

The Agent acts as the platform's "Eyes and Ears" on the ground. It is designed for minimal resource footprint and maximum reliability.

- **Real-time Telemetry**: Streams logs, metrics, and process events with microsecond latency.
- **Secure Tunneling**: Establishes a hardened outbound-only connection to the Aegis Core.
- **Auto-Discovery**: Identifies running containers and services within the local namespace.

```mermaid
graph LR
    Target[Target Apps] -- "Logs/Trace" --> Agent[Aegis Agent (Rust)]
    Agent -- "gRPC / mTLS" --> Nginx[Nginx Ingress]
    Nginx -- "Batch Write" --> Ingest[Ingest Worker]
```

---

## 🛠️ Tech Stack & Performance

| Component | Technology | Version |
|---|---|---|
| Language | **Rust** | 1.75+ |
| Async Runtime | **Tokio** | 1.x |
| Transport | **gRPC (Tonic)** | 0.x |
| Performance | < 20MB RSS | — |

---

## 🔐 Security & DevSecOps

- **Mutual TLS**: The Agent **requires** a valid client certificate to speak to the Ingest layer. No exceptions.
- **Zero-Privilege**: Designed to run as a non-root User/Group.
- **Static Binary**: Compiled into a single, dependency-free binary to reduce the attack surface.
- **No Inbound**: The Agent never opens listening ports; it only performs outbound streaming.

---

## 🐳 Deployment (Docker)

```bash
docker pull ghcr.io/aegis-ai/aegis-agent:latest

# Run as a unprivileged container
docker run -d \
  --name aegis-agent \
  --read-only \
  --cap-drop=ALL \
  --user 1000:1000 \
  -e INGEST_HOST="ingest.aegis.ai:443" \
  -v /etc/aegis/certs:/etc/certs:ro \
  ghcr.io/aegis-ai/aegis-agent:latest
```

---

## 🛠️ Development

```bash
# Build the binary
cargo build --release

# Run unit tests
cargo test
```

---

*Aegis AI — Telemetry & Collection — 2026*
