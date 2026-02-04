# Build Stage
FROM rust:1.76-slim-bookworm as builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache dependencies
COPY Cargo.toml .
# Create a dummy main to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm src/main.rs

# Build the actual application
COPY . .
# Touch main.rs to ensure rebuild
RUN touch src/main.rs
RUN cargo build --release

# Runtime Stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y libssl-dev ca-certificates curl && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /usr/src/app/target/release/panel /app/panel

# Copy assets and templates
COPY assets /app/assets
COPY templates /app/templates
COPY migrations /app/migrations
# Copy setup scripts if needed
# COPY scripts /app/scripts

# Environment setup
ENV RUST_LOG=info
ENV PORT=3000
ENV ADMIN_PATH=/admin

# Expose port
EXPOSE 3000

# Run the application
CMD ["/app/panel"]
