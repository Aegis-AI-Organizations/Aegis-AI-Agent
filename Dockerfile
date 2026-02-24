# Optimized Dockerfile for Rust project
# Stage 1: Build dependencies and binary
FROM rust:1.76-alpine AS builder
# Install musl tools for static compilation
RUN apk add --no-cache musl-dev
WORKDIR /app
# Copy the source code
COPY . .
# Build release binary (statically linked for musl) and install to /usr/local/cargo/bin/
RUN cargo install --path . --root /usr/local/ || echo "Build failed or no Cargo.toml"

# Stage 2: Minimal Runtime
FROM alpine:3.19
RUN apk add --no-cache ca-certificates
# Copy only the compiled binary
COPY --from=builder /usr/local/bin/* /usr/local/bin/
# Replace 'app' with the exact name of your binary
CMD ["app"]
