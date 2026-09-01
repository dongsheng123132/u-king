# U-King Windows 一键下载安装 / 升级（命令行兜底）
#
# 为什么用命令行：个别客户的运营商线路会对某个下载域名做 SNI 阻断
# （TCP 通、443 通、ping 通，但一发 HTTPS 就「连接已重置」ERR_CONNECTION_RESET）。
# 不同客户被封的域名不一样，所以这条脚本依次尝试多条线路，哪条通用哪条；
# 顺手检测并补上 WebView2 运行时（Tauri 应用必需，Win10 老机/精简系统常缺，缺了会白屏）。
#
# 用法（PowerShell 粘贴回车）：
#   irm https://u-claw.org.cn/uking/dl.ps1 | iex

$ErrorActionPreference = 'Stop'

# 强制 UTF-8 输出。Windows PowerShell 5.1 默认按本地代码页（简中机器上是 GBK）
# 输出，本脚本里的中文会被糊成「å¾®è½¯」这类乱码 —— 客户看到乱码只会以为装崩了。
try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new() } catch {}

# 🔴 线路顺序必须与 src-tauri/src/installer.rs::VERSION_URLS 一致：
# u-claw.org.cn 国内可达优先。
#
# 旧版这里写的是 u-king.org → cloud.u-claw.org，**把已验证可达的
# u-claw.org.cn 整个漏在列表外**，而 CLAUDE.md 明写 cloud.u-claw.org 在部分
# 网络会被 GFW SNI reset。客户执行的命令是 `irm https://u-claw.org.cn/uking/dl.ps1`
# —— 脚本本身就是从 u-claw.org.cn 下来的，这条线 100% 通，却排不进下载列表，
# 于是被封的客户先撞 u-king.org、再撞一条已知被封的域名。
$VersionUrls = @(
  'https://u-claw.org.cn/uking/version.json',
  'https://cloud.u-claw.org/uking/version.json',
  'https://www.u-king.org/version.json'
)
$Sources = @(
  'https://u-claw.org.cn/download/',
  'https://cloud.u-claw.org/download/',
  'https://u-king.org/download/'
)
$File  = 'U-King-Setup.exe'
$Dst   = Join-Path ([Environment]::GetFolderPath('Desktop')) $File
$Force = ($args -contains '--force')

Write-Host ''
Write-Host '  U-King 一键下载安装' -ForegroundColor Cyan
Write-Host '  =========================================='
Write-Host ''

# ── 1. 查最新版本（多线路，任一条通即可）────────────────────────────────
Write-Host '  正在查询最新版本…' -ForegroundColor Gray
$latest = $null
foreach ($vu in $VersionUrls) {
  try {
    $m = Invoke-RestMethod $vu -Headers @{ 'User-Agent' = 'U-King-Install' } -TimeoutSec 12
    if ($m.version) { $latest = $m.version; break }
  } catch {
    # 静默换下一条。原始异常是本地化英文/中文混排，糊给客户只会吓人。
  }
}

# ── 2. 读本机已装版本（NSIS 写在卸载表里）───────────────────────────────
$installed = $null
$regPaths = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
# 🔴 这里必须精确匹配 'U-King'（= tauri.conf.json 的 productName），不能用 'U-King*'。
# 通配符会捞到同前缀的**别的产品**：本机实测 'u-king爆款复制机' -like 'U-King*' 为 True
# （PowerShell 的 -like 不区分大小写），配上 -First 1 就是「注册表枚举顺序说了算」——
# 挑中那条 0.1.3 的话，下面「已是最新」永远不成立，客户每次都重下重装，
# 而这正是本次改动要修的东西，等于静默失效。
foreach ($p in $regPaths) {
  $e = Get-ItemProperty $p -ErrorAction SilentlyContinue |
       Where-Object { $_.DisplayName -eq 'U-King' } |
       Select-Object -First 1
  if ($e) { $installed = $e.DisplayVersion; break }
}

if ($latest) {
  Write-Host ("  最新版本：v{0}" -f $latest) -ForegroundColor Green
} else {
  # 查不到版本不是致命错 —— 下载线路可能仍然通。如实说明后继续往下走，
  # 不假装「已是最新」也不假装「有新版」。
  Write-Host '  查不到最新版本（版本服务不可达），继续尝试下载…' -ForegroundColor Yellow
}

if ($installed) {
  Write-Host ("  已装版本：v{0}" -f $installed) -ForegroundColor Gray
  if ($latest -and $installed -eq $latest -and -not $Force) {
    Write-Host ''
    Write-Host ("  ✅ 已经是最新版（v{0}），无需重装。" -f $installed) -ForegroundColor Green
    # 🔴 别写成「irm … | iex 后加 --force」——那个说法两头都不成立：
    # ① iex 是把脚本文本当表达式求值，脚本里的 $args 取到的是调用方作用域的，
    #    从控制台跑恒为空（实测 args=0），所以 --force 永远读不到；
    # ② `iex "<脚本文本> --force"` 是把 --force 拼进源码，PowerShell 当场报
    #    「一元运算符 -- 后面缺少表达式」的解析错误。
    # 唯一能传进参数的写法是先编成 scriptblock 再带参调用：
    Write-Host '     要强制重装，复制这一整行执行：' -ForegroundColor DarkGray
    Write-Host '     &([scriptblock]::Create((irm https://u-claw.org.cn/uking/dl.ps1))) --force' -ForegroundColor DarkGray
    Write-Host ''
    # `irm | iex` 从「运行」框起的窗口会在脚本返回瞬间关闭，不停一下客户根本看不到这行字。
    Write-Host '  按任意键关闭…' -ForegroundColor DarkGray
    try { [void][System.Console]::ReadKey($true) } catch {}
    return
  }
  if ($latest -and -not $Force) {
    Write-Host ("  正在升级 v{0} → v{1} …" -f $installed, $latest) -ForegroundColor Yellow
  }
} else {
  Write-Host '  本机未安装，执行全新安装…' -ForegroundColor Gray
}

# ── 3. 下载（多线路轮询）────────────────────────────────────────────────
Write-Host ''
$ok = $false
# 🔴 先下到 .part 临时文件、校验通过再原子改名，绝不让 curl 直接写 $Dst。
# 旧写法 `-o $Dst`：curl 一开口就把目标文件截断，而被劫持的线路返回的是
# 200 + 一张 HTML 错误页 —— 下面的体积校验确实拦住了它、也会换下一条线，但客户
# 桌面上**上一次下好的那个 10MB 安装包已经被覆盖成几十字节的 HTML 了**，
# 而全线失败时结尾还在说「你已安装的 vN 保持不变」（app 是没变，被毁的是那个包）。
# 宪法第 10 条：绝不静默覆盖你没创建的东西，写入要先留底 + 原子 + 可回滚。
# .part 放在 $Dst 同目录（不是 $env:TEMP）——同卷才保证 Move-Item 是原子改名，
# 跨卷会退化成「边拷边覆盖」，等于把刚修掉的坑换个地方再挖一遍。
$tmp = $Dst + '.' + [Guid]::NewGuid().ToString('N').Substring(0,8) + '.part'
foreach ($base in $Sources) {
  $url = $base + $File
  $host_ = ([Uri]$url).Host
  Write-Host ("  [线路] 尝试 {0} …" -f $host_) -NoNewline
  try {
    if (Test-Path $tmp) { Remove-Item $tmp -Force -ErrorAction SilentlyContinue }
    # curl.exe（Win10+ 内置）比 Invoke-WebRequest 对 SNI 阻断更易绕过，且能走 IP
    & curl.exe -fL -sS --connect-timeout 10 -m 900 -o $tmp $url 2>$null
    if ($LASTEXITCODE -eq 0 -and (Test-Path $tmp) -and (Get-Item $tmp).Length -gt 500000) {
      Write-Host ' 成功 ✓' -ForegroundColor Green
      $ok = $true
      break
    } else {
      Write-Host ' 不通，换下一条' -ForegroundColor Yellow
    }
  } catch {
    Write-Host ' 不通，换下一条' -ForegroundColor Yellow
  }
}

if ($ok) {
  try { Move-Item -LiteralPath $tmp -Destination $Dst -Force }
  catch {
    # 目标被占用（比如上一个安装包正开着）也不算白下 —— 告诉客户包在哪，别丢掉。
    Write-Host ("  写入 {0} 失败：{1}" -f $Dst, $_.Exception.Message) -ForegroundColor Yellow
    Write-Host ("  安装包已下好，在：{0}" -f $tmp) -ForegroundColor Yellow
    $Dst = $tmp
  }
} elseif (Test-Path $tmp) {
  Remove-Item $tmp -Force -ErrorAction SilentlyContinue
}

if (-not $ok) {
  Write-Host ''
  Write-Host '  所有线路都没下成。可能是网络问题，请稍后再试，或联系客服。' -ForegroundColor Red
  if ($installed) {
    Write-Host ("  你已安装的 v{0} 保持不变，可以正常使用。" -f $installed) -ForegroundColor Gray
  }
  return
}

$sizeMB = [math]::Round((Get-Item $Dst).Length / 1MB, 2)
Write-Host ("  下载完成：{0}（{1} MB）" -f $Dst, $sizeMB) -ForegroundColor Green

# ── 4. 检测 WebView2（Tauri 必需）。注册表里查不到就装微软在线引导包。────
function Test-WebView2 {
  $keys = @(
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
  )
  foreach ($k in $keys) {
    $v = (Get-ItemProperty -Path $k -Name pv -ErrorAction SilentlyContinue).pv
    if ($v -and $v -ne '0.0.0.0') { return $true }
  }
  return $false
}

if (Test-WebView2) {
  Write-Host '  WebView2 运行时：已就位 ✓' -ForegroundColor Green
} else {
  Write-Host '  WebView2 运行时：缺失，正在自动安装（U-King 必需，否则会白屏）…' -ForegroundColor Yellow
  $wv = Join-Path $env:TEMP 'MicrosoftEdgeWebview2Setup.exe'
  $wvOk = $false
  # 微软官方源 + 国内镜像兜底。
  # 🔴 u-claw.org.cn 上这个文件在 /uking/ 下，不在站点根 —— 根路径是 404，
  # 已用 HEAD 实测：/uking/ 与 u-king.org 根下的是同一个文件（均 1,688,832 字节）。
  foreach ($wvUrl in @('https://go.microsoft.com/fwlink/p/?LinkId=2124703',
                       'https://u-claw.org.cn/uking/MicrosoftEdgeWebview2Setup.exe',
                       'https://u-king.org/MicrosoftEdgeWebview2Setup.exe')) {
    try {
      & curl.exe -fL -sS --connect-timeout 10 -m 300 -o $wv $wvUrl 2>$null
      if ($LASTEXITCODE -eq 0 -and (Test-Path $wv)) { $wvOk = $true; break }
    } catch {}
  }
  if ($wvOk) {
    Start-Process -FilePath $wv -ArgumentList '/silent','/install' -Wait
    Write-Host '  WebView2 安装完成 ✓' -ForegroundColor Green
  } else {
    Write-Host '  WebView2 自动安装失败，安装 U-King 后若白屏，请手动装：' -ForegroundColor Yellow
    Write-Host '  https://developer.microsoft.com/microsoft-edge/webview2/' -ForegroundColor Yellow
  }
}

# ── 5. 拉起安装包 ───────────────────────────────────────────────────────
Write-Host ''
Write-Host '  正在打开安装包，按提示下一步即可…' -ForegroundColor Cyan
Start-Process -FilePath $Dst
