# Aegis AI Host Agent

The Rust-based Host Agent operates at the kernel-level through eBPF to monitor malicious activities. In the MVP v2 architecture, the Agent is configured dynamically by the orchestrator but streams its telemetry independently to avoid Gateway bottlenecks.
