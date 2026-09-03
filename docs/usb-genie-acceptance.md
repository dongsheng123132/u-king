# USB AI Genie 验收记录

> 本记录只写已经观察到的证据；未列出的项目不是默认通过。

## 2026-09-03 · Windows 真盘、无凭据冒烟

构建物：本仓 `src-tauri/target/debug/u-king-mini.exe`（包含本次 USB Genie Action Core）。

固定上游输入：PicoClaw `v0.3.1` Windows x86_64 zip，22,385,616 字节，SHA-256：
`c76e4b9f3f95137b8cb58229e756538b12ed0fee0e4301a01b531626bf740af1`。

执行链路（均通过）：

```text
inspect（target_id + target_root）
→ deploy(credential_ref=none)
→ verify
→ launch
```

| 介质 | 文件系统 | 结果 |
| --- | --- | --- |
| F: `我的 AI 精灵` | NTFS | 通过。根目录非 U-King 启动器文件 hash 对照无变化；未生成 `.security.yml`；`install.json` 与 runtime/launcher hash 写入成功。 |
| E: `U-CLAW-TEST` | exFAT | 通过。同一身份化链路与启动分支完成。 |

## 2026-09-03 · 首次制作下载路径与 FAT32 拒绝

- 在空白 FAT32 测试盘 G: 上，U-King 成功下载固定 PicoClaw `v0.3.1` archive 到宿主缓存，
  文件大小为 22,385,616 字节并命中 pinned SHA-256；下载阶段不写 U 盘。
- 随后的 FAT32 runtime 目录提交被 Windows 拒绝访问。该证据促使 P1 明确支持范围收敛为
  NTFS/exFAT：现在在**任何写入之前**返回 `unsupported_filesystem`，界面禁用制作并提示换盘。
- 首次制作失败路径同时改为清理本次新建的受管目录；不会写完成标记，也不会把半成品显示为已安装。
- 以 E: exFAT 已管理工具盘重放省略 `zip_path` 的同一 Action：复用已校验缓存，制作和随后 `verify`
  七项检查全部通过。这是 GUI 首次制作与 CLI 使用同一 Action Core 的实机证据。

冒烟脚本：`tools/usb-genie/smoke.ps1`。脚本会通过 inspect 获取 `target_id`，不能只传盘符；并强制 CLI JSON 模式。

## 尚未通过发布闸门的项目

- 凭据随盘携带的真实网关单轮、工具调用和丢盘处置演练；
- 空间不足、写保护、提交中拔盘、运行中更新的真盘故障注入；
- PicoClaw 上游升级切换与完整 rollback；
- FAT32 不作为可制作文件系统；
- OpenClaw、ClawX 各自的便携落盘取证、适配器与真盘验收。

这些项目完成前，产品可提供“已验证工具盘的检查与启动”，但不应宣布为完整发布。
