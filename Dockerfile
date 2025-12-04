# Build stage
FROM rust:1 AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests and build dependencies first for better layer caching
COPY Cargo.toml ./
# Note: Cargo.lock is gitignored in this project, so dependencies will resolve at build time
# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src
COPY web ./web

# Build the application with actual source
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -r -u 10001 -s /usr/sbin/nologin appuser

WORKDIR /app

# Copy the binary from builder with proper ownership
COPY --from=builder --chown=appuser:appuser /app/target/release/sort_it_now /app/sort_it_now

# Expose the default port
EXPOSE 8080

# Set environment variables with defaults
ENV SORT_IT_NOW_API_HOST=0.0.0.0
ENV SORT_IT_NOW_API_PORT=8080

# Switch to non-root user
USER appuser

# Run the binary
CMD ["/app/sort_it_now"]
