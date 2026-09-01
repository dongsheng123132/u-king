#!/usr/bin/env bash
# U-King 发版回归测试 —— 一键红绿报告。
#
# 把"发版前人工把 selfcheck 跑一遍核对输出"固化成可重复的断言脚本。
# 纯 bash + jq，无 bats 依赖。在 Git Bash 跑。
#
# 用法:
#   bash scripts/regression.sh              # 全量(含真装 Hermes、网络实测)
#   bash scripts/regression.sh --quick      # 跳过真装 Hermes 和外网链路检查(只跑本地+沙箱)
#   UKING_TEST_KEY=sk-... bash scripts/regression.sh   # 用有余额的 key 实测余额/连通
#
# 退出码: 0 = 全绿; 非0 = 有失败(失败数)
#
# 设计依据: 2026-06-17 调研结论——app 已有 --selfcheck JSON 输出,
# GUI E2E(tauri-driver/Playwright)是反模式,bash 包 selfcheck 最省力。见 memory uking-099-released。
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

QUICK=0
[ "${1:-}" = "--quick" ] && QUICK=1

# ── 颜色 + 计数 ──────────────────────────────────────────
G='\033[32m'; R='\033[31m'; Y='\033[33m'; D='\033[2m'; N='\033[0m'
PASS=0; FAIL=0; SKIP=0
ok()   { PASS=$((PASS+1)); echo -e "  ${G}✓${N} $1"; }
no()   { FAIL=$((FAIL+1)); echo -e "  ${R}✗ $1${N}"; [ -n "${2:-}" ] && echo -e "    ${D}$2${N}"; }
skip() { SKIP=$((SKIP+1)); echo -e "  ${Y}–${N} $1 ${D}(skipped)${N}"; }
sec()  { echo; echo -e "${D}── $1 ──${N}"; }

PROXY="http://127.0.0.1:7897"   # 开发机走代理才能连外网;脚本里需要时显式用
cn()   { curl -s --proxy "$PROXY" "$@"; }   # 经代理(模拟能上网的环境)

EXE="src-tauri/target/release/u-king-mini.exe"
SANDBOX="$ROOT/.regression-sandbox"

echo "════════════════════════════════════════════"
echo "  U-King 发版回归测试  $([ $QUICK = 1 ] && echo '(quick)')"
echo "════════════════════════════════════════════"

# ── 1. 版本号四处同步 ────────────────────────────────────
sec "1. 版本号四处同步"
V_TS=$(grep -oE 'APP_VERSION = "[^"]+"' src/version.ts | grep -oE '[0-9.]+')
V_PKG=$(jq -r .version package.json)
V_TAURI=$(jq -r .version src-tauri/tauri.conf.json)
V_CARGO=$(grep -oE '^version = "[^"]+"' src-tauri/Cargo.toml | grep -oE '[0-9.]+')
echo -e "    ${D}version.ts=$V_TS package.json=$V_PKG tauri.conf=$V_TAURI Cargo.toml=$V_CARGO${N}"
if [ "$V_TS" = "$V_PKG" ] && [ "$V_PKG" = "$V_TAURI" ] && [ "$V_TAURI" = "$V_CARGO" ]; then
  ok "四处版本号一致 ($V_TS)"
else
  no "版本号不一致" "ts=$V_TS pkg=$V_PKG tauri=$V_TAURI cargo=$V_CARGO"
fi
VERSION="$V_CARGO"

# ── 2. 关键 bug 修复仍在源码里(防回退) ───────────────────
sec "2. 已修 bug 防回退(源码断言)"
grep -q 'hard \* 500_000' src-tauri/src/providers.rs \
  && ok "双重扣减修复在 (providers.rs hard*500_000, 无 -used/100)" \
  || no "双重扣减修复被回退了!"
# endpoint 不能有裸 api.u-claw.org(注释除外) 在 provider 配置/余额查询里
BARE=$(grep -nE 'https://api\.u-claw\.org/' src-tauri/src/providers.rs | grep -v '\.org\.cn' | grep -vE '^\s*//|⚠️|裸|GFW|实测' || true)
[ -z "$BARE" ] && ok "providers.rs 无裸 api.u-claw.org(全 cn 镜像)" \
              || no "providers.rs 仍有裸 api.u-claw.org" "$BARE"
grep -q 'persist_python_scripts_path' src-tauri/src/installer.rs \
  && ok "Hermes PATH 持久化修复在 (persist_python_scripts_path)" \
  || no "Hermes PATH 修复丢失!"
grep -q 'u-claw.org.cn/uking/version.json' src-tauri/src/installer.rs \
  && ok "version 首选源是 cn 镜像" \
  || no "version 首选源不是 cn 镜像"

# ── 3. cargo check ──────────────────────────────────────
sec "3. Rust 编译检查"
if (cd src-tauri && cargo check -q 2>/tmp/cargo-check.log); then
  ok "cargo check 通过"
else
  no "cargo check 失败" "$(tail -5 /tmp/cargo-check.log)"
fi

# ── 4. exe 存在 + 版本自报 ──────────────────────────────
sec "4. 构建产物 + selfcheck"
if [ -f "$EXE" ]; then
  ok "exe 存在 ($EXE)"
else
  no "exe 不存在 — 先跑 pnpm tauri build"
  echo; echo "提前结束(无 exe 无法跑沙箱测试)"; exit 1
fi

# ── 5. 沙箱驱动切换(不碰真实配置) ────────────────────────
sec "5. 沙箱配置写出(UKING_TEST_HOME, 不动你真实 ~/.claude)"
rm -rf "$SANDBOX"; mkdir -p "$SANDBOX"
RES="$SANDBOX/result.json"
HTTP_PROXY="$PROXY" HTTPS_PROXY="$PROXY" \
  UKING_TEST_HOME="$SANDBOX" \
  ${UKING_TEST_KEY:+UKING_TEST_KEY="$UKING_TEST_KEY"} \
  "$EXE" --selfcheck "$RES" >/dev/null 2>&1
# 沙箱里四套工具配置 endpoint 该全是 cn 镜像
chk_cfg() {  # $1=文件 $2=描述 $3=grep正则(期望存在)
  if [ -f "$1" ] && grep -qE "$3" "$1"; then ok "$2"; else no "$2" "缺 $3 in $1"; fi
}
chk_cfg "$SANDBOX/.claude/settings.json"        "Claude → cn 镜像"   'api\.u-claw\.org\.cn'
chk_cfg "$SANDBOX/.codex/config.toml"           "Codex → cn 镜像"    'api\.u-claw\.org\.cn/v1'
chk_cfg "$SANDBOX/.codex/config.toml"           "Codex model=正确id" 'model = "gpt-5\.3-codex"'
chk_cfg "$SANDBOX/.openclaw/openclaw.json"      "OpenClaw → cn 镜像" 'api\.u-claw\.org\.cn'
chk_cfg "$SANDBOX/ClawX/clawx-providers.json"   "ClawX → cn 镜像"    'api\.u-claw\.org\.cn'
# selfcheck 自报版本
SV=$(jq -r '.version // empty' "$RES" 2>/dev/null)
[ "$SV" = "$VERSION" ] && ok "selfcheck 自报版本 $SV" || no "selfcheck 版本 $SV ≠ $VERSION"

# ── 6. 余额实测(需 UKING_TEST_KEY,验证双重扣减) ──────────
sec "6. 余额实测(双重扣减验证)"
if [ -n "${UKING_TEST_KEY:-}" ]; then
  CHARGED=$(jq -r '.driver_test.charged // .charged // empty' "$RES" 2>/dev/null)
  TOKENS=$(jq -r '[.. | objects | .tokens? // empty] | max // 0' "$RES" 2>/dev/null)
  if [ "$CHARGED" = "true" ] && [ "${TOKENS:-0}" -gt 0 ] 2>/dev/null; then
    ok "余额正常 charged=true, tokens=$TOKENS (双重扣减未回退)"
  else
    no "余额异常 charged=$CHARGED tokens=$TOKENS" "若是有余额的 key 报 false,说明双重扣减回退了"
  fi
else
  skip "余额实测(设 UKING_TEST_KEY=sk-... 启用)"
fi

# ── 7. 发版链路可达(外网) ────────────────────────────────
sec "7. 发版链路可达(国内客户源)"
if [ $QUICK = 1 ]; then
  skip "发版链路检查(--quick)"
else
  chk_url() {  # $1=url $2=描述 $3=期望版本(可选)
    local code body
    code=$(cn -o /tmp/u.json -w '%{http_code}' -m 25 "$1" 2>/dev/null)
    if [ "$code" = "200" ]; then
      if [ -n "${3:-}" ]; then
        body=$(jq -r '.version // empty' /tmp/u.json 2>/dev/null)
        [ "$body" = "$3" ] && ok "$2 → 200, v$body" || no "$2 → 200 但版本=$body 期望 $3"
      else ok "$2 → 200"; fi
    else no "$2 → HTTP $code"; fi
  }
  chk_url "https://u-claw.org.cn/uking/version.json"       "自升级检测(cn镜像)"  "$VERSION"
  chk_url "https://u-claw.org.cn/download/U-King-Setup.exe" "国内下载(cn镜像)"
  chk_url "https://u-claw-updates.oss-cn-shenzhen.aliyuncs.com/uking/U-King-Setup.exe" "自升级下载(OSS)"
fi

# ── 8. 服务端模型别名(GPT/Codex 不再 503) ───────────────
sec "8. 服务端模型 503 别名"
if [ $QUICK = 1 ] || [ -z "${UKING_TEST_KEY:-}" ]; then
  skip "模型 503 检查(需 UKING_TEST_KEY 且非 --quick)"
else
  for m in gpt-5-mini gpt-5 o4-mini; do
    code=$(cn -o /dev/null -w '%{http_code}' -m 30 https://api.u-claw.org.cn/v1/chat/completions \
      -H "Authorization: Bearer $UKING_TEST_KEY" -H "Content-Type: application/json" \
      -d "{\"model\":\"$m\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":5}" 2>/dev/null)
    [ "$code" = "200" ] && ok "$m → 200(别名生效)" || no "$m → HTTP $code(可能 503,别名失效)"
  done
fi

# ── 9. 真装 Hermes 验 PATH(全量才跑) ────────────────────
sec "9. Hermes 安装 + PATH 持久化"
if [ $QUICK = 1 ]; then
  skip "真装 Hermes(--quick)"
else
  echo -e "    ${D}真装 Hermes 较慢(下载 Python+pip),约 3-5 分钟...${N}"
  HTTP_PROXY="$PROXY" HTTPS_PROXY="$PROXY" UKING_TEST_INSTALL=hermes \
    "$EXE" --selfcheck "$SANDBOX/hermes.json" >/dev/null 2>&1
  HEX="$HOME/.uking/runtime/python/Scripts/hermes.exe"
  [ -f "$HEX" ] && ok "hermes.exe 已装" || no "hermes.exe 未装出"
  if powershell -NoProfile -Command "[Environment]::GetEnvironmentVariable('Path','User')" 2>/dev/null | grep -qi 'uking.*python.*Scripts\|uking.\runtime.\python'; then
    ok "Scripts 目录已持久化进用户 PATH(Bug3 修复生效)"
  else
    no "Scripts 未进用户 PATH(Bug3 回退)"
  fi
fi

# ── 收尾清理 + 报告 ──────────────────────────────────────
rm -rf "$SANDBOX"
echo
echo "════════════════════════════════════════════"
echo -e "  结果:  ${G}$PASS 通过${N}  ${R}$FAIL 失败${N}  ${Y}$SKIP 跳过${N}"
echo "════════════════════════════════════════════"
[ $FAIL -eq 0 ] && { echo -e "${G}全绿,可发版 ✓${N}"; exit 0; } \
                || { echo -e "${R}有失败,先修再发 ✗${N}"; exit $FAIL; }
