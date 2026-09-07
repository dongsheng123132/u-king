# OpenClaw Windows 绿色预览包

固定 Windows x64 完整包由 `scripts/build-openclaw-portable.mjs` 从显式 U-King exe 和已核验 runtime 缓存组装。`portable.json` 开启受管便携模式；钱包、日志和时间轴在包内 `U-King/data/uking`，OpenClaw 资料在 `U-King/OpenClaw`。

默认配置只做 schema 验证，不调用模型；显式 probe 动作才可能消耗额度。充值只打开浏览器页面，不创建支付。预览包支持查看、备份、恢复和轮换设备钱包，但不提供“移除本机钱包”；该跨层重置流程保留给完整桌面产品。恢复或轮换密钥后，需停止并重新启动 OpenClaw，使新密钥生效。OpenClaw 2026.8.1 的真实 config CLI 已在 NTFS 与 exFAT 基线验证；构建期仅为 workspace 写入器加入受管 Windows fs-safe sidecar，未开启时保留上游严格策略。每个包均附带哈希、许可证和运行时说明。
