#!/usr/bin/env bash
# 绿色测试版 —— 只为「在本机开一个真 Tauri 窗口点一点」，不用碰客户/自己正开着的那个 U-King。
#
# 单实例锁是按 **tauri identifier** 做的：identifier 一样 = 第二个实例被顶回去。
# 所以这里用 `--config` 覆盖 identifier（**不改仓库里的 tauri.conf.json**，那是公共汇合点，
# 多终端并行时改它必冲突），编出一个跟正式版井水不犯河水的壳。
#
# 顺带的好处/代价，都得知道：
#  - identifier 变了 → **webview 的 localStorage 是全新的**。正好用来验「第一次进来长什么样」
#    （发现卡片、新手引导这类只出现一次的东西），但也意味着它看不到正式版存的界面偏好。
#  - `~/.uking`、`~/.claude`、`~/.openclaw` 这些是**按路径**找的，不跟 identifier 走 ——
#    所以绿色版读到的是**同一份真实数据**。要隔离就自己套 `UKING_TEST_HOME`。
#  - 用 debug profile（不开 LTO），编译快很多；**别拿它量性能**
#    （实测同一份数据 debug 2905ms / release 751ms，快慢关系还会反过来）。
#
# 用法：bash scripts/build-green-test.sh   → 打印产物路径
set -euo pipefail
cd "$(dirname "$0")/.."

ID="${GREEN_ID:-org.uking.greentest}"
echo "[green] identifier=$ID（正式版是 tauri.conf.json 里那个，两边互不挡）"

pnpm tauri build --debug --no-bundle --config "{\"identifier\":\"$ID\"}"

EXE="src-tauri/target/debug/u-king-mini.exe"
[ -f "$EXE" ] || { echo "[green] 没找到产物 $EXE"; exit 1; }
echo "[green] OK -> $EXE"
echo "[green] 直接双击或： \"$EXE\""
