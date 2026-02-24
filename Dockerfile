# Stage 1: Build dependencies and binary
# Upgrade to 1.85-alpine to support newer Rust editions
FROM rust:1.85-alpine AS builder
# Install musl tools for static compilation
RUN apk add --no-cache musl-dev
WORKDIR /app
# Copy the source code (Cleaned from review comments)
COPY . .
# Build release binary
RUN cargo install --path . --root /usr/local/

# Stage 2: Minimal Runtime
FROM alpine:3.19
RUN apk add --no-cache ca-certificates
# Copy only the compiled binary (Cleaned from review comments)
# NOTE: Replace 'aegis-ai-agent' if your binary has a different name
COPY --from=builder /usr/local/bin/aegis-ai-agent /usr/local/bin/aegis-ai-agent
# Set the command to run your binary
CMD ["aegis-ai-agent"]
