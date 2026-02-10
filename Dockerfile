# --- 阶段 1: 编译 ---
FROM rust:slim-trixie AS builder
WORKDIR /app
# 安装编译所需的依赖库
RUN apt-get update && apt-get install -y pkg-config libssl-dev g++ && rm -rf /var/lib/apt/lists/*
COPY . .

ENV SQLX_OFFLINE=true
# 运行编译
RUN cargo build --release

# --- 阶段 2: 运行 ---
FROM ubuntu:24.04
WORKDIR /app
# 安装运行时必要的库
RUN apt-get update && apt-get install -y libssl3 ca-certificates openssl && rm -rf /var/lib/apt/lists/*

# 从编译阶段拷贝二进制文件
COPY --from=builder /app/target/release/data-dict-backend .
# 拷贝模型缓存（确保你本地这个路径下已经有模型文件了）
COPY ./model ./model

# 设置离线模式变量
ENV HF_HUB_OFFLINE=1
ENV RUST_LOG=info
# 设置后端监听端口
EXPOSE 3000

CMD ["./data-dict-backend"]
