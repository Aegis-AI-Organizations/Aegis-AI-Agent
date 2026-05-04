#!/bin/bash
set -e

# Configuration
AGENT_USER="aegis-agent"
INSTALL_DIR="/usr/local/bin"
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
    mv "$BINARY_NAME" "$INSTALL_DIR/"
    chmod 755 "$INSTALL_DIR/$BINARY_NAME"
else
    echo "Error: Binary $BINARY_NAME not found in current directory."
    exit 1
fi

# Create systemd service
echo "Creating systemd service..."
cat <<EOF > /etc/systemd/system/$SERVICE_NAME
[Unit]
Description=Aegis AI Agent
After=network.target

[Service]
Type=simple
User=$AGENT_USER
WorkingDirectory=/var/lib/aegis-agent
ExecStart=$INSTALL_DIR/$BINARY_NAME
Restart=always
RestartSec=10
# Security hardening
NoNewPrivileges=true
PrivateTmp=true
DeviceAllow=/dev/null rw
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target
EOF

# Create working directory
mkdir -p /var/lib/aegis-agent
chown $AGENT_USER:$AGENT_USER /var/lib/aegis-agent
chmod 700 /var/lib/aegis-agent

# Reload systemd and start service
systemctl daemon-reload
systemctl enable $SERVICE_NAME
systemctl start $SERVICE_NAME

echo "Aegis AI Agent installed and started successfully."
