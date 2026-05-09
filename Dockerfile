# ===== 构建阶段 =====
# 使用 bullseye (Debian 11, glibc 2.31) 规避 buildkit v0.8 的 clone3 seccomp 限制
FROM docker.m.daocloud.io/library/rust:1.87-slim-bullseye AS builder

WORKDIR /app

# crates.io 国内镜像
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
FROM docker.m.daocloud.io/library/debian:bullseye-slim

WORKDIR /app
COPY --from=builder /app/target/release/gongs-credit .

EXPOSE 5000

CMD ["./gongs-credit"]
