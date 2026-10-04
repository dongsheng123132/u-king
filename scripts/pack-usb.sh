#!/usr/bin/env bash
# pack-usb.sh —— 把构建产物组装成「U 盘根目录」结构。
#
# 产出 dist-usb/：
#   U-King.exe        ← 根目录主程序（双击即用）
#   U-King/           ← 资源子目录（图标 / 说明）
#     icon.ico
#     说明.txt
#   自述-先看我.txt    ← 给客户的一句话说明
#
# 把 dist-usb/ 里的所有东西拷到 U 盘根目录即可。
set -e

HERE="$(cd "$(dirname "$0")/.." && pwd)"
REL="$HERE/src-tauri/target/release"
OUT="$HERE/dist-usb"

EXE="$REL/u-king-mini.exe"
if [ ! -f "$EXE" ]; then
  echo "[ERROR] 未找到 $EXE，请先 pnpm tauri build"
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT/U-King"

# 根目录主程序（重命名为对客户友好的 U-King.exe）
cp "$EXE" "$OUT/U-King.exe"

# —— U 盘护符：写入密钥 uking.key（丢盘不能跑的弱保护，见 src-tauri/src/guard.rs）——
# 把密钥盖进 $OUT/U-King/uking.key（藏在子目录、随手党不会拷）。所有 U 盘同一个 key，零逐盘工序。
# 用 exe 自己的 --arm-usb 写（token 与 exe 内嵌盐一致，绝不写错）。
"$OUT/U-King.exe" --arm-usb "$OUT" >/dev/null 2>&1 && echo "[OK] 已写入 U 盘护符密钥 -> $OUT/U-King/uking.key" \
  || echo "[WARN] 写护符密钥失败（不影响其余组装）"

# 安全闸：确认这个 exe 真的是「U 盘口味」（带护符）。忘了 --features usb-guard 会把无保护的
# 下载版 exe 烧进 U 盘 → 护符形同虚设。这里探测一下，不是就大声告警（不阻断，允许你有意出无护符盘）。
if "$OUT/U-King.exe" --guard-check 2>/dev/null | grep -q '"enabled":true'; then
  echo "[OK] 该 exe 带 U 盘护符（enabled=true）"
else
  echo "[⚠⚠⚠] 该 exe 【无护符】！烧进 U 盘将无法防白嫖。"
  echo "       要出「U 盘版」请先： pnpm tauri build --features usb-guard  再跑本脚本。"
fi

# —— 硬闸：前端到底内嵌了没有 ——
# 2026-09-04 烧掉过一次线上发布（usb-ai-genie v0.1.0）：用 `cargo build --release` 出的 exe
# 少了 tauri 的 custom-protocol feature，dist/ 没被打进去，运行时回落到 devUrl
# http://localhost:1430，客户机上双击 = ERR_CONNECTION_REFUSED。上面那道护符闸【拦不住它】
# ——无护符只是少个保护，前端没内嵌是根本打不开，所以这条是 exit 1 不是告警。
#
# 判据取内嵌资源表里的 `assets/index`（vite 产物固定命名）。实测：`pnpm tauri build` 出的
# exe 命中 4 次，`cargo build --release` 出的命中 0 次。不要拿 "localhost:1430" 反过来断言
# ——它在好坏两种 exe 里都在（tauri.conf 作为配置数据一起内嵌），据此判断会得到假绿灯。
ASSET_HITS="$(grep -a -o "assets/index" "$OUT/U-King.exe" 2>/dev/null | wc -l | tr -d ' ')"
if [ "${ASSET_HITS:-0}" -gt 0 ]; then
  echo "[OK] 前端已内嵌（assets/index 命中 $ASSET_HITS 次）"
else
  echo "[FAIL] 该 exe 【前端没内嵌】（assets/index 命中 0 次）——烧进 U 盘客户双击只会看到"
  echo "       ERR_CONNECTION_REFUSED。这是 cargo build --release 的产物，不是发布产物。"
  echo "       请改用： pnpm tauri build --features usb-guard  重新构建后再跑本脚本。"
  exit 1
fi

# 资源
cp "$HERE/src-tauri/icons/icon.ico" "$OUT/U-King/icon.ico" 2>/dev/null || true

# 图文使用说明书（双击用浏览器打开，离线可看）
cp "$HERE/website/usb-guide.html" "$OUT/使用说明书.html" 2>/dev/null || \
  echo "[WARN] 未找到 website/usb-guide.html，跳过说明书"

cat > "$OUT/自述-先看我.txt" <<'TXT'
U-King 个人 AI 操作系统 · U 盘版

【新手三步，照着做】
  1) 双击根目录的  U-King.exe  打开管理界面。
  2) 点「一键安装 ClawX」（🦞 图形版 AI 助手，主力工具），它会自动帮你装好、配好 AI。
  3) 打开 ClawX，若 Windows 弹「是否允许访问网络」，点【允许访问】，就能用了。

【不会用？】
  双击  使用说明书.html  （图文教程，浏览器打开），三步学会用 AI。

【还有这些】
  · 一键安装到本地 —— 装到电脑后拔了 U 盘也能用，桌面自动建快捷方式
  · 右键目录菜单   —— 任意文件夹右键多一项「用 U-King 打开」
  · 右下角托盘常驻 —— 关窗口不退出，像 360 一样守在右下角

说明：U-King 需要联网下载并安装 AI 工具；配置文件写在你的用户目录（不动系统盘）。
TXT

cp "$OUT/自述-先看我.txt" "$OUT/U-King/说明.txt"

echo "[OK] U 盘结构已生成：$OUT"
echo ""
ls -la "$OUT"
echo ""
echo "把 $OUT 里的全部内容拷到 U 盘根目录即可。"
