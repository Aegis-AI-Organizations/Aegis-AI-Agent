# 🚀 Quickstart : Agent Hôte Aegis

Connectez un hôte à Aegis en quelques minutes. L'Agent s'enregistre une fois avec
un token de déploiement, puis diffuse sa télémétrie en sortie sur mTLS.

---

## Prérequis

1. Accès owner ou admin au Dashboard Aegis.
2. Un token de déploiement valide au format `ag_<43+ caractères URL-safe>`
   (**Paramètres → Token agent** dans le Dashboard ; affiché une seule fois).
3. Accès HTTPS sortant de l'hôte vers le Cœur Aegis.

---

## Option 1 — Linux autonome (systemd)

Une seule commande télécharge le binaire statique, installe un service systemd
durci et injecte le token :

```bash
curl -sL "https://api.aegis-ai.fr/install.sh?token=<VOTRE_TOKEN_DE_DEPLOIEMENT>" | sudo bash
```

Gérer le service :

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
  -e DEPLOYMENT_TOKEN="<VOTRE_TOKEN_DE_DEPLOIEMENT>" \
  -e AGENT_NAME="$(hostname)-aegis-agent" \
  -v aegis-agent-state:/var/lib/aegis-agent \
  ghcr.io/aegis-ai/aegis-agent:latest
```

## Option 3 — Kubernetes (DaemonSet Helm)

```bash
helm install aegis-agent ./chart --set token=<VOTRE_TOKEN_DE_DEPLOIEMENT>
```

La valeur `token` est obligatoire ; l'installation échoue sans elle.

---

## Vérifier

Dans la vue sécurité du Dashboard, le panneau de statut de l'agent doit se mettre
à jour :

- `Agents déployés` augmente après l'enregistrement ;
- `Actifs` augmente après le premier heartbeat ;
- `Vu pour la dernière fois` se rafraîchit à chaque heartbeat.

Contrôle de santé local :

```bash
curl http://127.0.0.1:8081/health
```

---

## Développement local

```bash
export GATEWAY_URL="http://localhost:8080"
export DEPLOYMENT_TOKEN="ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg"
export AGENT_NAME="local-agent-01"
export AGENT_ALLOW_HTTP="true"   # local uniquement
cargo run
```

---

## Sécurité du token

Le token de déploiement n'est affiché qu'une seule fois. En cas de fuite, faites
une rotation ou révoquez-le depuis **Paramètres → Token agent** avant de déployer
de nouveaux agents ; les agents déjà enregistrés continuent avec leur propre
`agent_secret`.

---

*Ingénierie du capteur hôte Aegis AI — 2026*
