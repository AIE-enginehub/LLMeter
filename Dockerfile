FROM docker.m.daocloud.io/library/rust:1.87-slim-bookworm AS builder

WORKDIR /app

# rust 国内源
RUN mkdir -p /root/.cargo && \
    printf '[source.crates-io]\nreplace-with = "rsproxy"\n\n[source.rsproxy]\nregistry = "sparse+https://rsproxy.cn/index/"\n' > /root/.cargo/config.toml

# 禁止并行编译
ENV CARGO_BUILD_JOBS=1
ENV RUSTFLAGS="-C codegen-units=1"

# 先缓存依赖
COPY Cargo.toml Cargo.lock ./

RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release -j 1 && \
    rm -rf src

# 复制正式代码
COPY . .

# 正式构建
RUN cargo build --release -j 1

# runtime
FROM docker.m.daocloud.io/library/debian:bookworm-slim

WORKDIR /app

COPY --from=builder /app/target/release/gongs-credit /app/gongs-credit

EXPOSE 8080

CMD ["./gongs-credit"]