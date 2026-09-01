#!/usr/bin/env bash
# make-usb-master.sh —— 一键把「U 盘版（带护符）」刷进发盘母目录。
#
# 以后发 U 盘版就跑这一条：
#     bash scripts/make-usb-master.sh                 # 默认母目录
#     bash scripts/make-usb-master.sh "D:/别的母目录"  # 指定目录
#
# 它做四件事：
#   1) 编「带护符」exe（pnpm tauri build --features usb-guard --no-bundle）
#   2) 覆盖母目录根 U-King.exe
#   3) 在母目录写隐藏解锁钥匙 U-King/uking.key（所有 U 盘同一把，整目录拷进 U 盘即可）
#   4) 刷新 06-售后与版本/SHA256SUMS.txt 里 U-King.exe 的哈希
#
# 原理：护符焊在 U 盘版 exe 里（拷到哪都带着），uking.key 是钥匙。没盘=没钥匙=打不开。
# 官网下载版走普通 `pnpm tauri build`（无护符，零门槛），跟本脚本无关。
set -e

HERE="$(cd "$(dirname "$0")/.." && pwd)"
REL_EXE="$HERE/src-tauri/target/release/u-king-mini.exe"

# 发盘母目录（可用第 1 个参数覆盖）
DEFAULT_MASTER="${UKING_MASTER:-$HOME/Desktop/uking-master}"
MASTER="${1:-$DEFAULT_MASTER}"

if [ ! -d "$MASTER" ]; then
  echo "[ERROR] 母目录不存在：$MASTER"
  echo "        用法：bash scripts/make-usb-master.sh \"你的母目录路径\""
  exit 1
fi

echo "==> 目标母目录：$MASTER"
echo "==> [1/4] 编带护符 exe（release，可能几分钟）..."
( cd "$HERE" && pnpm tauri build --features usb-guard --no-bundle )

if [ ! -f "$REL_EXE" ]; then
  echo "[ERROR] 没找到构建产物 $REL_EXE"
  exit 1
fi

# 安全闸：确认真的是带护符版
if ! "$REL_EXE" --guard-check 2>/dev/null | grep -q '"enabled":true'; then
  echo "[ERROR] 构建出来的 exe 竟然没护符（enabled≠true）—— 构建参数可能没带上 --features usb-guard，中止。"
  exit 1
fi

echo "==> [2/4] 覆盖母目录根 U-King.exe..."
# 先关掉**母目录里那一个**旧 exe，释放文件锁（Windows）。
#
# 🔴 绝不按裸镜像名杀（宪法：`taskkill /IM` 不区分同名进程）。老写法是
#     taskkill //F //IM "U-King.exe"
# 它会把**这台机器上所有**叫 U-King.exe 的进程一起端了 —— 包括你正插着用的 U 盘版、
# 客户机上装的那个。跟 `taskkill /IM Claude.exe` 连 Claude Code CLI 一起杀是同一个坑
#（见 backup.rs::LOCKING_IMAGE_NAMES），只是这次撞的是我们自己。
# 这里只认**完整路径相等**的那些 PID，逐个关。
MASTER_EXE_WIN="$(cygpath -w "$MASTER/U-King.exe" 2>/dev/null || echo "$MASTER/U-King.exe")"
powershell -NoProfile -Command "
  Get-CimInstance Win32_Process -Filter \"Name='U-King.exe'\" |
    Where-Object { \$_.ExecutablePath -ieq '$MASTER_EXE_WIN' } |
    ForEach-Object { Stop-Process -Id \$_.ProcessId -Force -ErrorAction SilentlyContinue }
" >/dev/null 2>&1 || true
sleep 1
cp -f "$REL_EXE" "$MASTER/U-King.exe"

echo "==> [3/4] 写隐藏解锁钥匙 U-King/uking.key..."
# 用 release exe 自己写（token 与 exe 内嵌盐一致），不启动母目录里的 exe（避免锁文件）
"$REL_EXE" --arm-usb "$MASTER" >/dev/null

echo "==> [4/4] 刷新 SHA256SUMS.txt 里 U-King.exe 的哈希..."
SUMS="$MASTER/06-售后与版本/SHA256SUMS.txt"
if [ -f "$SUMS" ]; then
  NEW_HASH="$(sha256sum "$MASTER/U-King.exe" | awk '{print $1}')"
  # 替换以 ` *U-King.exe` 结尾的那一行（前面 64 位十六进制哈希）
  sed -i -E "s#^[0-9a-f]{64} \*U-King\.exe\$#${NEW_HASH} *U-King.exe#" "$SUMS"
  echo "    U-King.exe SHA256 -> $NEW_HASH"
else
  echo "    [跳过] 没找到 $SUMS"
fi

echo ""
echo "==> 完成。核验："
echo "    护符状态: $("$MASTER/U-King.exe" --guard-check)"
echo "    钥匙文件: $MASTER/U-King/uking.key = $(cut -c1-16 "$MASTER/U-King/uking.key")…"
echo ""
echo "把整个母目录内容拷进 U 盘根目录即可（务必连 U-King/uking.key 一起）。"
echo "验一块盘好没好：在插着盘的电脑上跑  U-King.exe --guard-check  看 usb_present 是不是 true。"
