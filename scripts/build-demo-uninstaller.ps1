# 生成可单独拷到 U 盘/桌面的「U-King 演示卸载工具.exe」。
# 它与主程序共用清理后端，但只显示演示清场页面；不需要安装。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
  # 必须经 Tauri CLI 打包：它会把 dist 内嵌到 exe。直接 cargo build 会保留开发用 localhost，
  # 双击后就会出现“localhost 拒绝连接”。--no-bundle 只生成绿色 exe，不做安装包。
  pnpm tauri build --features demo-uninstaller --no-bundle

  $outDir = Join-Path $root 'dist-demo-uninstaller'
  New-Item -ItemType Directory -Force -Path $outDir | Out-Null
  Copy-Item -LiteralPath (Join-Path $root 'src-tauri\target\release\u-king-mini.exe') `
    -Destination (Join-Path $outDir 'U-King-演示卸载工具.exe') -Force
  Write-Host "已生成：$(Join-Path $outDir 'U-King-演示卸载工具.exe')"
}
finally {
  Pop-Location
}
