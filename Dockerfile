# ===== 阶段 1：依赖缓存层 =====
FROM docker.m.daocloud.io/library/rust:1.87-slim-bookworm AS chef

# 修复 Debian GPG 密钥过期问题并安装构建依赖
RUN rm -f /etc/apt/apt.conf.d/docker-clean \
    && apt-get update -o Acquire::Check-Valid-Until=false -o Acquire::AllowInsecureRepositories=true -o Acquire::AllowDowngradeToInsecureRepositories=true \
    && apt-get install -y --allow-unauthenticated debian-archive-keyring gnupg \
    && gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys 6ED0E7B82643E131 78DBA3BC47EF2265 F8D2585B8783D481 54404762BBB6E853 BDE6D2B9216EC7A8 \
    && gpg --export 6ED0E7B82643E131 78DBA3BC47EF2265 F8D2585B8783D481 54404762BBB6E853 BDE6D2B9216EC7A8 | apt-key add - \
    && apt-get update \
    && apt-get install -y pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 使用中科大 crates.io 镜像
RUN mkdir -p /usr/local/cargo/registry \
    && printf '[source.crates-io]\nreplace-with = "ustc"\n\n[source.ustc]\nregistry = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"\n\n[registries.ustc]\nindex = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"\n' \
    > /usr/local/cargo/config.toml

WORKDIR /app

# 先拷贝依赖清单，构建空壳项目以缓存依赖编译产物
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/.fingerprint/gongs-credit-*

# ===== 阶段 2：编译项目 =====
FROM chef AS builder

COPY src/ src/
COPY static/ static/
COPY migrations/ migrations/

RUN cargo build --release

# ===== 阶段 3：运行（从构建阶段拷贝 keyring 避免重复修复）=====
FROM docker.m.daocloud.io/library/debian:bookworm-slim

COPY --from=chef /etc/apt/trusted.gpg /etc/apt/trusted.gpg
COPY --from=chef /usr/share/keyrings/debian-archive-keyring.gpg /usr/share/keyrings/debian-archive-keyring.gpg

RUN rm -f /etc/apt/apt.conf.d/docker-clean \
    && apt-get update && apt-get install -y ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/gongs-credit .
COPY migrations/ migrations/

EXPOSE 5000

CMD ["./gongs-credit"]
