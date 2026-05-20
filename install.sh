#!/bin/bash
set -e

# Configuration
AGENT_USER="aegis-agent"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/aegis-agent"
VAR_DIR="/var/lib/aegis-agent"
BINARY_NAME="aegis-ai-agent"
SERVICE_NAME="aegis-agent.service"

echo "Installing Aegis AI Agent..."

# Create restricted user
if ! id "$AGENT_USER" &>/dev/null; then
    echo "Creating restricted user: $AGENT_USER"
    useradd --system --shell /usr/sbin/nologin --no-create-home "$AGENT_USER"
fi

# Move binary (assuming it's in the current directory)
if [ -f "$BINARY_NAME" ]; then
    cp "$BINARY_NAME" "$INSTALL_DIR/"
    chmod 755 "$INSTALL_DIR/$BINARY_NAME"
else
    echo "Error: Binary $BINARY_NAME not found in current directory."
    exit 1
fi

# Prepare config directory
mkdir -p "$CONFIG_DIR"
if [ ! -f "$CONFIG_DIR/agent.env" ]; then
    echo "Creating environment template at $CONFIG_DIR/agent.env"
    cat <<EOF > "$CONFIG_DIR/agent.env"
# Aegis AI Agent Configuration
GATEWAY_URL=https://api.aegis.ai
DEPLOYMENT_TOKEN=ag_<43+_URL_SAFE_CHARS>
INGEST_HOST=localhost
INGEST_PORT=7233
# Bind health checks to localhost by default. Change to 0.0.0.0 only if
# you intentionally need remote access and have appropriate network controls.
HEALTH_BIND_ADDR=127.0.0.1
HEALTH_PORT=8081
EOF
fi
chown -R root:root "$CONFIG_DIR"
chmod 755 "$CONFIG_DIR"
chmod 600 "$CONFIG_DIR/agent.env"

# Create working directory
mkdir -p "$VAR_DIR"
chown "$AGENT_USER:$AGENT_USER" "$VAR_DIR"
chmod 700 "$VAR_DIR"

# Create systemd service
echo "Creating systemd service..."
cat <<EOF > /etc/systemd/system/$SERVICE_NAME
[Unit]
Description=Aegis AI Agent
After=network.target

[Service]
Type=simple
User=$AGENT_USER
WorkingDirectory=$VAR_DIR
EnvironmentFile=$CONFIG_DIR/agent.env
ExecStart=$INSTALL_DIR/$BINARY_NAME
Restart=always
RestartSec=10

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
DeviceAllow=/dev/null rw
ProtectSystem=full
ProtectHome=true
CapabilityBoundingSet=
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX

[Install]
WantedBy=multi-user.target
EOF

# Reload systemd and start service
systemctl daemon-reload
systemctl enable $SERVICE_NAME

echo "-------------------------------------------------------"
echo "Aegis AI Agent installed successfully."
echo "CRITICAL: Update $CONFIG_DIR/agent.env with your credentials."
echo "Then run: systemctl start $SERVICE_NAME"
echo "-------------------------------------------------------"
