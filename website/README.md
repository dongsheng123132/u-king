# u-king.org 网站骨架

静态站，结构对齐 u-claw.org 的 Vercel 部署方式。

## 内容

| 路径 | 作用 |
|---|---|
| `index.html` | 下载落地页（黑金主题，下载按钮指向 `/download/...`） |
| `skills/install-windows.json` | **服务器下发的安装 skill**。app 内嵌同源兜底版；此文件 `version` 调大后，所有客户端下次安装自动用新清单（改 npm 源、加修复步骤、换 Node 版本都不用发版） |
| `download/`（待放置） | `U-King-Setup.exe`（NSIS 安装包）与 `U-King.exe`（绿色单 exe），从 `src-tauri/target/release/` 拷入 |

## 部署

站点为纯静态；部署由维护者的发布流程完成，见仓库根 README。
