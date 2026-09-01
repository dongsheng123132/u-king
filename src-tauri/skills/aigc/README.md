# U-King AIGC 技能包

把「AI 作图 + AI 视频」打包成给任意 AI 工具 CLI 调用的技能包。由 U-King「AI 技能包」页一键导出。

## 里面有什么

```
uking-aigc/
├── SKILL.md              # 技能说明（AI 读这个就知道怎么调）
├── README.md             # 本文件
└── scripts/
    ├── gen-image.mjs     # 文生图 / 图生图
    ├── gen-video.mjs     # 文生/图生视频（提交前持久化 + 幂等防重复扣费 + 中断续跑/下载）
    └── gen-batch.mjs     # 批量 / 多进程：一次并发出多张图 / 多条视频
```

零 npm 依赖，只用 Node 内置模块 + 系统 curl。

## 快速测试

```bash
cd uking-aigc
node scripts/gen-image.mjs --prompt "一只橘猫宇航员，电影感" --out cat.png --json
node scripts/gen-video.mjs --prompt "橘猫在月球慢跑，星空" --out cat.mp4 --json
```

成功会在当前目录生成 `cat.png` / `cat.mp4`，并打印 `{"ok":true,"file":"..."}`。

## API Key 从哪来

脚本自动按 `--key` > 环境变量 `XIAPAN_API_KEY` > `~/.uking/device.json` 取。装了 U-King 的电脑直接有，无需手动配。没充值去 https://u-claw.org.cn/recharge 。

## 装到各工具

见 `SKILL.md` 末尾「把本技能装进各 AI 工具」。一句话：把整个 `uking-aigc/` 文件夹拷进目标工具的 skills 目录，重启即可。
