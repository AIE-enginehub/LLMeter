# ===== 阶段 1：构建 =====
FROM rust:1.87-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY static/ static/

RUN cargo build --release

# ===== 阶段 2：运行 =====
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/gongs-credit .
COPY --from=builder /app/static/ static/
COPY migrations/ migrations/

EXPOSE 3000

CMD ["./gongs-credit"]
