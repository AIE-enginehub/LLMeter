# ===== 构建阶段 =====
FROM docker.m.daocloud.io/library/rust:1.87-slim-bookworm AS builder

WORKDIR /app

# crates.io 国内镜像（字节跳动 rsproxy）
RUN mkdir -p /root/.cargo \
    && printf '[source.crates-io]\nreplace-with = "rsproxy"\n\n[source.rsproxy]\nregistry = "sparse+https://rsproxy.cn/index/"\n' \
    > /root/.cargo/config.toml

# 先缓存依赖编译产物
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/.fingerprint/gongs-credit-*

# 复制源码并正式构建
COPY src/ src/
COPY static/ static/
COPY migrations/ migrations/
RUN cargo build --release

# ===== 运行阶段 =====
# static/ 和 migrations/ 已通过 rust-embed / include_str! 编译进二进制
# sqlx 使用 tls-rustls 不依赖系统 OpenSSL，无需安装任何 apt 包
FROM docker.m.daocloud.io/library/debian:bookworm-slim

WORKDIR /app
COPY --from=builder /app/target/release/gongs-credit .

EXPOSE 5000

CMD ["./gongs-credit"]
