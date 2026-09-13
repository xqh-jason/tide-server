# 后端多阶段构建：builder（rustc 全量编译）→ runtime（bookworm-slim 瘦身）
# 产物：tide-server（API 服务）+ migration（数据库迁移 CLI，entrypoint 先跑）
# 版本锁定：rust:1.96-slim 与本地工具链（rustc 1.96.1）一致，避免依赖 MSRV 漂移

# ---- builder：编译期工具链 ----
FROM rust:1.96-slim AS builder
# sea-orm / sqlx 依赖 openssl 与编译期工具链
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
# 根 crate 与 migrations 是两个独立包（非 workspace）：分两次构建
RUN cargo build --release -p tide-server --locked
WORKDIR /app/migrations
RUN cargo build --release --locked

# ---- runtime：只保留可执行文件与配置文件 ----
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata curl \
    && rm -rf /var/lib/apt/lists/* \
    && ln -snf /usr/share/zoneinfo/Asia/Shanghai /etc/localtime \
    && echo "Asia/Shanghai" > /etc/timezone

WORKDIR /app
ENV TIDE_ENV=production \
    TZ=Asia/Shanghai

COPY --from=builder /app/target/release/tide-server ./
COPY --from=builder /app/migrations/target/release/migration ./
COPY config.toml ./config.toml
COPY docker/entrypoint.sh ./entrypoint.sh
RUN chmod +x entrypoint.sh && mkdir -p uploads

EXPOSE 8080
ENTRYPOINT ["./entrypoint.sh"]
