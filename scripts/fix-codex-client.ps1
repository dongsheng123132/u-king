# 客户机修复：把坏掉的新版 Codex 降级到兼容虾盘云的 0.80.0
# 用法（远程）：remote-agent.exe push pc-XXXX ./scripts/fix-codex-client.ps1 C:/temp/fix.ps1
#              remote-agent.exe exec --shell powershell pc-XXXX "powershell -File C:/temp/fix.ps1"
Write-Output "[1/3] 卸载当前 Codex..."
npm uninstall -g @openai/codex 2>&1 | Out-Null
Write-Output "[2/3] 安装兼容版 0.80.0（npmmirror 加速）..."
npm install -g @openai/codex@0.80.0 --registry=https://registry.npmmirror.com --no-fund --no-audit 2>&1 | Select-Object -Last 2
Write-Output "[3/3] 验证..."
$v = codex --version 2>&1
Write-Output "Codex 版本: $v"
if ($v -match "0.80") { Write-Output "OK 修复成功，现在 codex 能正常用了" } else { Write-Output "WARN 版本不对，请检查" }
