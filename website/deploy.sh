#!/usr/bin/env bash
# 官网发布薄入口。部署参数只存在于被 gitignore 的私有 JSON 配置中；默认是只读 plan。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec node "$ROOT/scripts/release-uking.mjs" "$@"
