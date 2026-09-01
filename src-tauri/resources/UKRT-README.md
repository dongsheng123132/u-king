# ukrt.exe 说明

本文件是 `ukrt.exe`（约 500KB，Windows CLI）的来源声明。

- 功能：AI 运行时的命令行入口，主程序构建时内嵌，运行时按需释放到 `~/.uking/ukrt/`。
- 源码：位于维护者私有的 ai-runtime 仓库（未随本仓库发布）。构建产物以二进制形式同步进 `resources/`，发版时更新。
- 许可证：与主程序一致，遵循仓库根目录的 Apache-2.0 LICENSE。
- 校验：发布说明（Release Notes）附 `ukrt.exe` 的 SHA-256。

如需源码审计该组件，请通过 [SECURITY.md](../../SECURITY.md) 渠道联系维护者。
