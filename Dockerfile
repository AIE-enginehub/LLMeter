# ===== 阶段 1：依赖缓存层 =====
FROM rust:1.87-slim-bookworm AS chef

# 使用国内 apt 镜像源（阿里云）
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# 使用中科大 crates.io 镜像
RUN mkdir -p /usr/local/cargo/registry \
    && cat > /usr/local/cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "ustc"

[source.ustc]
registry = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"

[registries.ustc]
index = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"
EOF

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

# ===== 阶段 3：运行 =====
FROM debian:bookworm-slim

RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/gongs-credit .
COPY migrations/ migrations/

EXPOSE 5000

CMD ["./gongs-credit"]
