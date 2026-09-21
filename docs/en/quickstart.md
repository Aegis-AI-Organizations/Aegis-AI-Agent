# 🚀 Quickstart: Aegis Host Agent

Connect a host to Aegis in a few minutes. The Agent registers once with a
deployment token, then streams telemetry outbound over mTLS.

---

## Prerequisites

1. Owner or admin access to the Aegis Dashboard.
2. A valid deployment token in the format `ag_<43+ URL-safe chars>`
   (**Settings → Agent token** in the Dashboard; shown once).
3. Outbound HTTPS from the host to the Aegis Core.

---

## Option 1 — Standalone Linux (systemd)

One command downloads the static binary, installs a hardened systemd unit, and
injects the token:

```bash
curl -sL "https://api.aegis-ai.fr/install.sh?token=<YOUR_DEPLOYMENT_TOKEN>" | sudo bash
```

Manage the service:

```bash
sudo systemctl status aegis-agent.service
sudo journalctl -u aegis-agent.service -f
```

## Option 2 — Docker

```bash
docker volume create aegis-agent-state

docker run -d \
  --name aegis-agent \
  --restart unless-stopped \
  --read-only \
  --cap-drop=ALL \
  -e GATEWAY_URL="https://api.aegis-ai.fr" \
  -e DEPLOYMENT_TOKEN="<YOUR_DEPLOYMENT_TOKEN>" \
  -e AGENT_NAME="$(hostname)-aegis-agent" \
  -v aegis-agent-state:/var/lib/aegis-agent \
  ghcr.io/aegis-ai/aegis-agent:latest
```

## Option 3 — Kubernetes (Helm DaemonSet)

```bash
helm install aegis-agent ./chart --set token=<YOUR_DEPLOYMENT_TOKEN>
```

The `token` value is required; the install fails without it.

---

## Verify

In the Dashboard security view, the agent status panel should update:

- `Agents deployed` increases after registration;
- `Active` increases after the first heartbeat;
- `Last seen` refreshes on every heartbeat.

Local health check:

```bash
curl http://127.0.0.1:8081/health
```

---

## Local development

```bash
export GATEWAY_URL="http://localhost:8080"
export DEPLOYMENT_TOKEN="ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg"
export AGENT_NAME="local-agent-01"
export AGENT_ALLOW_HTTP="true"   # local only
cargo run
```

---

## Token security

The deployment token is displayed only once. If it leaks, rotate or revoke it
from **Settings → Agent token** before deploying new agents; already-registered
agents keep working with their own `agent_secret`.

---

*Aegis AI Host Sensor Engineering — 2026*
