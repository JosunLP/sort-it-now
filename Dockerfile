# Build stage
FROM rust:1.85-slim AS builder

# Install required dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests
COPY Cargo.toml ./

# Copy source code
COPY src ./src
COPY web ./web

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/sort_it_now /app/sort_it_now

# Expose the default port
EXPOSE 8080

# Set environment variables with defaults
ENV SORT_IT_NOW_API_HOST=0.0.0.0
ENV SORT_IT_NOW_API_PORT=8080

# Run the binary
CMD ["/app/sort_it_now"]
