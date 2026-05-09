# =========================
# 1. planner
# =========================
FROM docker.m.daocloud.io/library/rust:1.87-slim-bookworm AS planner

# 使用阿里云 Debian 镜像
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# cargo 镜像
RUN mkdir -p /usr/local/cargo \
    && printf '[source.crates-io]\nreplace-with = "aliyun"\n\n[source.aliyun]\nregistry = "sparse+https://mirrors.aliyun.com/crates.io-index/"\n' \
    > /usr/local/cargo/config.toml

RUN cargo install cargo-chef

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

# =========================
# 2. builder
# =========================
FROM planner AS builder

COPY --from=planner /app/recipe.json recipe.json

# 依赖缓存
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo chef cook --release --recipe-path recipe.json

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release

# =========================
# 3. runtime
# =========================
FROM docker.m.daocloud.io/library/debian:bookworm-slim

RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/gongs-credit .

COPY migrations ./migrations
COPY static ./static

EXPOSE 5000

CMD ["./gongs-credit"]