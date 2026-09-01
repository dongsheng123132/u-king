#!/bin/sh
# U-King Mac 一键安装 / 升级
#
# 为什么用脚本装：浏览器下载的未签名 App 会被 macOS 标记 quarantine，
# 打开时弹「已损坏，无法打开」。curl 下载不带这个标记，再顺手清一次，
# 装进 /Applications 即开即用 —— 与 u-claw-toolkit-mac 的思路一致。
#
# 用法（终端粘贴回车）：
#   curl -fsSL https://u-claw.org.cn/uking/install-mac.sh | sh
#   curl -fsSL https://u-claw.org.cn/uking/install-mac.sh | sh -s -- --force   # 已最新也重装
set -e

# 🔴 线路顺序必须与 src-tauri/src/installer.rs::VERSION_URLS 一致：
# u-claw.org.cn 国内可达优先，cloud.u-claw.org 在部分网络会被 SNI reset。
# 客户能执行这个脚本，就证明第一条线是通的 —— 所以它必须排第一，
# 不能像旧版 dl.ps1 那样把已验证可达的域名漏在列表外。
VERSION_URLS="https://u-claw.org.cn/uking/version.json
https://cloud.u-claw.org/uking/version.json
https://www.u-king.org/version.json"

ZIP_BASES="https://u-claw.org.cn/download/
https://cloud.u-claw.org/download/
https://www.u-king.org/download/"

ZIP_NAME="U-King-Mac.zip"
APP="/Applications/U-King.app"
FORCE=""
[ "$1" = "--force" ] && FORCE=1

echo ""
echo "  U-King Mac 安装程序"
echo "  ──────────────────"

# ── 1. 查最新版本（多线路，任一条通即可）────────────────────────────────
# 不带 jq：客户机不一定有，用 grep/sed 抠 "version" 字段。
echo "  正在查询最新版本…"
LATEST=""
for u in $VERSION_URLS; do
  LATEST=$(curl -fsSL --connect-timeout 8 -m 15 "$u" 2>/dev/null \
    | grep -oE '"version"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 \
    | sed -E 's/.*"([^"]*)"$/\1/')
  if ! printf '%s\n' "$LATEST" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    LATEST=""
    continue
  fi
  break
done

# ── 2. 读本机已装版本 ───────────────────────────────────────────────────
INSTALLED=""
if [ -d "$APP" ] && [ -f "$APP/Contents/Info.plist" ]; then
  INSTALLED=$(defaults read "$APP/Contents/Info" CFBundleShortVersionString 2>/dev/null || true)
fi

if [ -z "$LATEST" ]; then
  # 三条线全不通。不猜原因（旧文案爱说「还在上传」，其实包一直都在），
  # 如实说网络不可达，并且明确告诉用户已装的那版没被动过。
  echo "  ⚠️  查不到最新版本（三条线路都没通，可能是网络或防火墙）。"
  if [ -n "$INSTALLED" ]; then
    echo "     你已安装的 v$INSTALLED 保持不变，可以正常使用。"
    echo "     手动下载：https://u-claw.org.cn/uking/"
    exit 0
  fi
  echo "     手动下载：https://u-claw.org.cn/uking/"
  exit 1
fi

echo "  最新版本：v$LATEST"

if [ -n "$INSTALLED" ]; then
  echo "  已装版本：v$INSTALLED"
  if [ "$INSTALLED" = "$LATEST" ] && [ -z "$FORCE" ]; then
    echo ""
    echo "  ✅ 已经是最新版（v$INSTALLED），无需重装。"
    echo "     要强制重装：curl -fsSL https://u-claw.org.cn/uking/install-mac.sh | sh -s -- --force"
    echo ""
    exit 0
  fi
  [ -z "$FORCE" ] && echo "  正在升级 v$INSTALLED → v$LATEST …"
  [ -n "$FORCE" ] && echo "  强制重装 v$LATEST …"
else
  echo "  本机未安装，执行全新安装…"
fi

# ── 3. 下载（多线路 + 重试 + 真进度条）──────────────────────────────────
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
ZIP="$TMP/uking.zip"

# 🔴 不写死体积。旧版硬编码「约 5.2MB」，包早就涨到 10.5MB 了还在这么说 ——
# 任何写死的数字都会过期，让 curl 的进度条自己报真实大小。
# 🔴 不用 `curl -C -` 断点续传：服务器返 5xx 时那段 HTML 已经落进文件，
# 续传会把正确的 zip 字节拼在 HTML 后面，长度对得上但解压必炸。
echo ""
OK=""
for base in $ZIP_BASES; do
  host=$(echo "$base" | sed -E 's#https?://([^/]+)/.*#\1#')
  printf "  [线路] 尝试 %s … " "$host"
  rm -f "$ZIP"

  # 先轻量探一下这条线通不通，通了再正式下载。
  # 为什么不直接让 curl 带 --retry 硬扛：DNS 解析不了 / 被 reset 的线路上，
  # 重试只是白等（实测 3 次重试 = 干等 6 秒），而且 curl 会把
  # `curl: (6) Could not resolve host` 这类英文原文糊到客户屏幕上 ——
  # 客户看不懂，只会觉得「装崩了」。探测失败就安静换线，别吓人。
  if ! curl -fsSI --connect-timeout 8 -m 20 -o /dev/null "${base}${ZIP_NAME}" 2>/dev/null; then
    echo "不通，换下一条"
    continue
  fi
  echo ""

  if curl -fL --progress-bar --retry 1 --retry-connrefused \
       --connect-timeout 15 -m 900 -o "$ZIP" "${base}${ZIP_NAME}"; then
    # 校验拿到的确实是 zip：被劫持/返错误页时文件也会「下载成功」，
    # 但那是一段 HTML。不验就会在 ditto 那步报一个看不懂的错。
    if [ -s "$ZIP" ] && ditto -x -k "$ZIP" "$TMP/probe" >/dev/null 2>&1; then
      rm -rf "$TMP/probe"
      OK=1
      break
    fi
    echo "         下回来的不是有效安装包（可能被网络劫持），换下一条线路"
  else
    echo "         下载中断，换下一条线路"
  fi
done

if [ -z "$OK" ]; then
  echo ""
  echo "  ❌ 三条线路都没下成。请稍后重试，或到 https://u-claw.org.cn/uking/ 手动下载。"
  if [ -n "$INSTALLED" ]; then
    echo "     你已安装的 v$INSTALLED 保持不变，可以正常使用。"
  fi
  exit 1
fi

# ── 4. 安装 ─────────────────────────────────────────────────────────────
echo ""
echo "  📦 正在安装到 $(dirname "$APP") …"
ditto -x -k "$ZIP" "$TMP"
if ! xattr -rc "$TMP/U-King.app" >/dev/null 2>&1; then
  echo "  ⚠️  未能清除下载标记；若打开被系统拦截，请按页面提示手动处理。"
fi

# 覆盖前先让正在跑的那份自己退出，否则替换正在运行的 .app 会让它当场崩、
# 而且用户看到的是「装完更坏了」。
# 🔴 用 osascript 按 app 身份请求退出，**不用 pkill -f**：
# CLAUDE.md 铁律 —— 绝不按裸名字批量杀进程（会误杀同名的 AI CLI 会话）。
if [ -d "$APP" ]; then
  osascript -e 'quit app "U-King"' >/dev/null 2>&1 || true
  sleep 1
  if command -v lsof >/dev/null 2>&1; then
    attempts=0
    while lsof +D "$APP" >/dev/null 2>&1; do
      [ "$attempts" -ge 5 ] && break
      attempts=$((attempts + 1))
      sleep 1
    done
    if lsof +D "$APP" >/dev/null 2>&1; then
      echo "  ⚠️  U-King 没能退出，为避免替换正在运行的应用，本次不覆盖安装。"
      echo "     请先手动退出 U-King（菜单栏图标 → 退出），再重新执行本条命令。"
      exit 1
    fi
  else
    # 极端环境没有 lsof 时不做「删了再说」，同样要求用户先退出（macOS 自带 lsof，此路极少走到）
    echo "  ⚠️  本机没有 lsof，无法确认 U-King 已退出。"
    echo "     请先手动退出 U-King（菜单栏图标 → 退出），再重新执行本条命令。"
    exit 1
  fi
fi

rm -rf "$APP"
ditto "$TMP/U-King.app" "$APP"

VERIFY=$(defaults read "$APP/Contents/Info" CFBundleShortVersionString 2>/dev/null || echo "?")
echo "  ✅ 安装完成：$APP （v$VERIFY）"
echo "  🚀 正在打开 U-King…"
open "$APP"
echo ""
