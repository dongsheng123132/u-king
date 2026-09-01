# Security Policy

## 报告漏洞

请**不要**通过公开 issue 报告安全漏洞。使用以下私下渠道之一：

- 邮件：security@hequbing.com
- GitHub Security Advisory（本仓库 Security 标签页 → Report a vulnerability）

请包含：复现步骤、受影响版本、可能的影响范围。我们会尽快确认收到。

## 响应承诺

本项目由单人维护，响应时间以此为前提：

- 密钥泄漏、凭据相关：目标 **72 小时**内首次响应（节假日可能顺延）
- 其他安全问题：按严重程度排期处理，会在 issue / 邮件里同步进展
- 若 7 天未收到任何回复，可在 GitHub Security Advisory 里补一条提醒

修复发布前，请勿公开披露细节。

## 本地数据存放位置

U-King 及其管理的工具在本地写入以下位置（Windows/Mac 路径形式类似）：

- `~/.claude`：Claude Code 配置与凭据
- `~/.codex`：Codex CLI 配置与凭据
- `~/.uking`：U-King 自身配置、日志、便携工具
- 各 AI 工具自身的会话 / 任务数据目录（由该工具自行管理）

以上均为本地文件，U-King 不会主动上传其内容。

## 构建产物校验

发布的 exe 附带 SHA-256 校验值，发布页面会同时给出文件与哈希。下载后可自行校验：

```bash
# Windows PowerShell
Get-FileHash .\u-king.exe -Algorithm SHA256

# macOS / Linux
shasum -a 256 u-king
```

比对结果与发布页面公布的哈希一致，即可确认产物未被篡改。
