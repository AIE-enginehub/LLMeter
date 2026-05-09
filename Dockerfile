# syntax=docker/dockerfile:1

# =========================
# Builder
# =========================
FROM docker.m.daocloud.io/library/rust:1.87-slim-bookworm AS builder

WORKDIR /app

# 使用阿里云 Debian 镜像
RUN sed -i 's|http://deb.debian.org|https://mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && sed -i 's|http://security.debian.org|https://mirrors.aliyun.com/debian-security|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
       pkg-config \
       libssl-dev \
       ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 先复制 Cargo 文件利用缓存
COPY Cargo.toml Cargo.lock ./

# 如果是 workspace 项目，把各 crate 的 Cargo.toml 也复制
# COPY crates/*/Cargo.toml crates/*/

# 创建空 src 避免首次 build 失败
RUN mkdir src && echo "fn main(){}" > src/main.rs

# 预编译依赖
RUN cargo build --release || true

# 删除假代码
RUN rm -rf src

# 复制完整项目
COPY . .

# 编译正式程序
RUN cargo build --release

# =========================
# Runtime
# =========================
FROM docker.m.daocloud.io/library/debian:bookworm-slim

WORKDIR /app

# runtime 只装必要依赖
RUN sed -i 's|http://deb.debian.org|https://mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && sed -i 's|http://security.debian.org|https://mirrors.aliyun.com/debian-security|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates \
       libssl3 \
    && rm -rf /var/lib/apt/lists/*

# 从 builder 拷贝二进制
# 把 gongs-credit 改成你的实际 binary 名称
COPY --from=builder /app/target/release/gongs-credit /app/gongs-credit

# 如果程序监听端口
EXPOSE 8080

# 启动
CMD ["/app/gongs-credit"]