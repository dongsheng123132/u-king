# Clean-room acceptance: exercise U-King's real installer for the three
# optional CLI agents. This intentionally does not authenticate to any provider.
$ErrorActionPreference = 'Continue'
try { chcp 65001 | Out-Null; [Console]::OutputEncoding=[Text.Encoding]::UTF8; $OutputEncoding=[Text.Encoding]::UTF8 } catch {}

$exe = 'C:\uking-test\U-King.exe'
if (-not (Test-Path $exe)) { Write-Output 'MISSING_UKING_EXE'; exit 2 }

foreach ($tool in @('pi', 'hermes', 'opencode')) {
  $out = "C:\uking-test\install-$tool.json"
  Remove-Item -LiteralPath $out -Force -ErrorAction SilentlyContinue
  $env:UKING_TEST_INSTALL = $tool
  Write-Output "=== $tool INSTALL ==="
  $p = Start-Process -FilePath $exe -ArgumentList "--selfcheck $out" -PassThru
  if (-not $p.WaitForExit(600000)) { try { $p.Kill() } catch {}; Write-Output "$tool TIMEOUT"; continue }
  Write-Output "$tool SELFHECK_EXIT=$($p.ExitCode)"
  if (Test-Path $out) {
    $j = Get-Content -Raw -Encoding UTF8 $out | ConvertFrom-Json
    Write-Output ("$tool INSTALL_RESULT=" + ($j.install_test.result | ConvertTo-Json -Compress -Depth 6))
    Write-Output ("$tool LOG_TAIL=" + (($j.install_test.log_tail | Select-Object -Last 5) -join ' | '))
  } else { Write-Output "$tool NO_RESULT_JSON" }

  $env:PATH = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine')
  $cmd = Get-Command $tool -ErrorAction SilentlyContinue
  Write-Output "$tool COMMAND=$($cmd.Source)"
  if ($cmd) { & $tool --version 2>&1 | Select-Object -First 3 | ForEach-Object { Write-Output "$tool VERSION=$_" } }
}
Remove-Item Env:UKING_TEST_INSTALL -ErrorAction SilentlyContinue
