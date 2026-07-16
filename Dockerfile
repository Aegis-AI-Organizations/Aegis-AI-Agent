# Stage 1: Build binary
FROM rust:1.88-alpine AS builder
RUN apk add --no-cache musl-dev ca-certificates g++ cmake
WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl --bins

# Stage 2: Ultra-secure Minimal Runtime
FROM scratch
# Copy CA certificates for HTTPS requests
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
# Copy the compiled static binary
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/aegis-ai-agent /aegis-ai-agent
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/aegis-redact /aegis-redact
# Run as non-privileged is handled by the orchestrator/systemd,
# but entrypoint is set to the binary
ENTRYPOINT ["/aegis-ai-agent"]
