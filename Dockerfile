FROM docker.m.daocloud.io/library/rust:1.87-slim-bookworm AS builder

WORKDIR /app

# 使用国内 cargo 镜像
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

RUN mkdir -p /root/.cargo && \
    printf '[source.crates-io]\nreplace-with = "rsproxy"\n\n[source.rsproxy]\nregistry = "sparse+https://rsproxy.cn/index/"\n' > /root/.cargo/config.toml

# 先复制依赖文件
COPY Cargo.toml Cargo.lock ./

# 创建假项目缓存依赖
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/*

# 复制正式代码
COPY . .

# 编译
RUN cargo build --release

# =========================
# runtime
# =========================
FROM scratch

WORKDIR /app

COPY --from=builder /app/target/release/gongs-credit /app/gongs-credit

EXPOSE 8080

CMD ["/app/gongs-credit"]