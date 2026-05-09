FROM docker.m.daocloud.io/library/rust:1.87-slim-bookworm AS planner

# 修复 Debian key 问题 + 阿里云源
RUN rm -f /etc/apt/apt.conf.d/docker-clean \
    && sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
       -o Acquire::AllowInsecureRepositories=true \
       -o Acquire::AllowDowngradeToInsecureRepositories=true \
       -o APT::Get::AllowUnauthenticated=true \
    && apt-get install -y --allow-unauthenticated \
       ca-certificates \
       pkg-config \
       libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Rust crates 镜像
RUN mkdir -p /usr/local/cargo \
    && printf '[source.crates-io]\nreplace-with = "aliyun"\n\n[source.aliyun]\nregistry = "sparse+https://mirrors.aliyun.com/crates.io-index/"\n' \
    > /usr/local/cargo/config.toml

# 安装 cargo-chef
RUN cargo install cargo-chef

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

# =========================
# builder
# =========================
FROM planner AS builder

COPY --from=planner /app/recipe.json recipe.json

RUN cargo chef cook --release --recipe-path recipe.json

COPY . .

RUN cargo build --release

# =========================
# runtime
# =========================
FROM docker.m.daocloud.io/library/debian:bookworm-slim

RUN rm -f /etc/apt/apt.conf.d/docker-clean \
    && sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
       -o Acquire::AllowInsecureRepositories=true \
       -o Acquire::AllowDowngradeToInsecureRepositories=true \
       -o APT::Get::AllowUnauthenticated=true \
    && apt-get install -y --allow-unauthenticated \
       ca-certificates \
       libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/gongs-credit .

COPY migrations ./migrations
COPY static ./static

EXPOSE 5000

CMD ["./gongs-credit"]