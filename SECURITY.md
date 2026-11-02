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

每个 release 都附带一个 `SHA256SUMS` 文件，逐行是 `<哈希>  <文件名>`（`sha256sum` 输出格式），覆盖该 release 里的 exe。下载后先校验、再安装：

```bash
# macOS / Linux / WSL / Windows Git Bash：直接按清单校验
shasum -a 256 -c SHA256SUMS      # macOS
sha256sum -c SHA256SUMS          # Linux / WSL / Windows Git Bash

# Windows PowerShell：没有 -c 可用，逐个比对
Get-FileHash .\U-King-Setup.exe -Algorithm SHA256
Get-Content .\SHA256SUMS
```

哈希与清单一致，即可确认产物在下载途中未被篡改（本 release 由 `.github/workflows/release.yml` 在 GitHub Actions 的 `windows-latest` 上构建并生成清单）。
