# ===== 构建阶段 =====
# 使用 bullseye (Debian 11, glibc 2.31) 规避 buildkit v0.8 的 clone3 seccomp 限制
# 升级 Rust 版本到 1.88-slim-bullseye，以满足依赖库的要求
FROM docker.m.daocloud.io/library/rust:1.88-slim-bullseye AS builder

WORKDIR /app

# 安装编译所需的系统依赖 (pkg-config 和 libssl-dev)
# 替换 apt 源为阿里云源加速国内下载
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list \
    && sed -i 's|security.debian.org/debian-security|mirrors.aliyun.com/debian-security|g' /etc/apt/sources.list \
    && apt-get update \
    && apt-get install -y pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 官方 Rust 镜像的 CARGO_HOME 是 /usr/local/cargo，所以配置要写在这里
# 使用字节跳动 (rsproxy) 的 sparse 镜像源，如果 ustc 也不稳定可以换这个
# 增加网络超时时间，防止下载大包时超时
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true \
    CARGO_NET_RETRY=3 \
    CARGO_HTTP_TIMEOUT=120

RUN mkdir -p /usr/local/cargo \
    && printf '[source.crates-io]\nreplace-with = "rsproxy"\n\n[source.rsproxy]\nregistry = "sparse+https://rsproxy.cn/index/"\n' \
    > /usr/local/cargo/config.toml

# 复制源码和依赖文件
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY static/ static/
COPY migrations/ migrations/

# 使用 BuildKit 缓存加速后续构建，避免每次都重新下载和编译依赖
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    # 重试机制：如果 cargo build 失败，则重试最多 3 次
    cargo build --release || cargo build --release || cargo build --release \
    && cp /app/target/release/llmeter /app/llmeter

# ===== 运行阶段 =====
FROM docker.m.daocloud.io/library/debian:bullseye-slim

# 安装运行时的系统依赖 (OpenSSL 动态库和 CA 证书，供 reqwest 等使用)
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list \
    && sed -i 's|security.debian.org/debian-security|mirrors.aliyun.com/debian-security|g' /etc/apt/sources.list \
    && apt-get update \
    && apt-get install -y libssl1.1 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
# 从 builder 阶段复制编译好的二进制文件
COPY --from=builder /app/llmeter .
# 因为启用了 rust-embed 的 debug-embed 特性，运行时需要读取实际文件
COPY static/ static/

EXPOSE 5000

CMD ["./llmeter"]
