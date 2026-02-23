# 🛡️ Aegis AI - Agent

**Project ID:** AEGIS-CORE-2026

## 🏗️ System Architecture & Role
The **Aegis AI Agent** is the edge collection module deployed directly within the **Client Infrastructure (Target)**. It acts as the primary sensor, securely capturing logs and telemetry in real-time, and securely streaming them to the Aegis Core Cloud.

* **Tech Stack:** Rust (Tokio for Async, Tonic for gRPC)
* **Performance SLO:**
  * CPU Usage < 10% (Single Core)
  * Memory Usage < 2GiB (RSS)
  * Ingestion Throughput: 10,000 Events/Sec (EPS)
* **Architecture Justification:** Zero Garbage Collection delays, minimal static binary footprint (< 10MB), and absolute Memory Safety guarantee.

## 🔐 Security & DevSecOps Mandates
* **No Plain-Text Secrets:** Passwords and keys exist only in memory, injected dynamically via Infisical/HashiCorp Vault (`tmpfs`). The use of `.env` files is STRICTLY FORBIDDEN.
* **Network Communication:** The Agent pushes data over a secure **gRPC (HTTP/2) Stream** encapsulated in strict **mTLS**. Relying on "Deny-All" default external policies.

## 🐳 Docker Deployment
The Agent is packaged as an immutable Docker image, intended to be deployed as a DaemonSet or sidecar agent within client clusters.

```bash
docker pull ghcr.io/aegis-ai/aegis-agent:latest

# Deployment wrapped with dynamic secret injection
infisical run --env=prod -- docker run -d \
  --name aegis-agent \
  --read-only \
  --cap-drop=ALL \
  --security-opt no-new-privileges:true \
  --user 10001:10001 \
  -e INFISICAL_TOKEN=$INFISICAL_TOKEN \
  -v /var/log/client:/var/log/client:ro \
  ghcr.io/aegis-ai/aegis-agent:latest
```
