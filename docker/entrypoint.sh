#!/usr/bin/env sh
# 容器入口：默认先执行数据库迁移，再启动 API 服务。
# 可覆盖行为：
#   - RUN_MIGRATIONS=0  跳过迁移（多副本扩容场景由外部统一执行）
#   - DATABASE_URL      迁移 CLI 认领的连接串（未设时回落 TIDE_DATABASE__URL）
set -e

mkdir -p uploads

if [ "${RUN_MIGRATIONS:-1}" != "0" ]; then
  echo "==> running database migrations"
  # 迁移 CLI（sea-orm-migration）读 DATABASE_URL；容器统一经 TIDE_DATABASE__URL 注入
  if [ -z "${DATABASE_URL:-}" ]; then
    DATABASE_URL="${TIDE_DATABASE__URL:-}"
    export DATABASE_URL
  fi
  if [ -z "$DATABASE_URL" ]; then
    echo "!! DATABASE_URL / TIDE_DATABASE__URL 均未设置，跳过迁移（服务可能因缺表启动失败）" >&2
  else
    ./migration up
  fi
fi

echo "==> starting API server"
exec ./tide-server
