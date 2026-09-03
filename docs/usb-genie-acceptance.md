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

冒烟脚本：`tools/usb-genie/smoke.ps1`。脚本会通过 inspect 获取 `target_id`，不能只传盘符；并强制 CLI JSON 模式。

## 尚未通过发布闸门的项目

- 凭据随盘携带的真实网关单轮、工具调用和丢盘处置演练；
- 空间不足、写保护、提交中拔盘、运行中更新的真盘故障注入；
- PicoClaw 上游升级切换与完整 rollback；
- OpenClaw、ClawX 各自的便携落盘取证、适配器与真盘验收。

这些项目完成前，产品可提供“已验证工具盘的检查与启动”，但不应宣布为完整发布。
