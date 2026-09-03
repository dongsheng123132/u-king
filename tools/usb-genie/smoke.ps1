param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$TargetRoot,
  [Parameter(Mandatory = $true)][string]$ZipPath,
  [switch]$Launch
)

$ErrorActionPreference = 'Stop'
function Invoke-GenieAction([string]$Id, [hashtable]$Payload, [bool]$Write = $false) {
  $inputFile = Join-Path ([IO.Path]::GetTempPath()) ("usb-genie-{0}.json" -f [guid]::NewGuid())
  try {
    # Windows PowerShell 5.1 lacks the `utf8NoBOM` encoding name.  The CLI
    # accepts JSON without a BOM, so use the .NET encoder explicitly on both
    # PowerShell 5.1 and 7 instead of making the smoke test host-dependent.
    [IO.File]::WriteAllText($inputFile, ($Payload | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
    # The smoke harness is a machine caller: force the documented JSON/stdout
    # contract so an interactive child console cannot be mistaken for a result.
    $args = @('action', 'run', $Id, '--input-file', $inputFile, '--json', '--no-input')
    if ($Write) { $args += '--yes' }
    $raw = & $Exe @args
    if ($LASTEXITCODE -ne 0) { throw "action $Id failed: $raw" }
    # Some Windows console hosts emit a standalone `.` while a child is being
    # attached. The action result itself is the final JSON object; reject any
    # other shape rather than piping every line into ConvertFrom-Json.
    $json = @($raw | Where-Object { $_ -is [string] -and $_.TrimStart().StartsWith('{') } | Select-Object -Last 1)
    if ($json.Count -ne 1) { throw "action $Id returned no JSON result: $($raw -join [Environment]::NewLine)" }
    return $json[0] | ConvertFrom-Json
  } finally { Remove-Item -LiteralPath $inputFile -Force -ErrorAction SilentlyContinue }
}

$inspect = Invoke-GenieAction -Id 'runtime.usb_genie.inspect' -Payload @{}
Write-Host "inspect: $($inspect.targets.Count) removable target(s)"
$target = @($inspect.targets | Where-Object { $_.target_root -ieq $TargetRoot }) | Select-Object -First 1
if (-not $target) { throw "target is not the currently inspected removable root: $TargetRoot" }
$identity = @{ target_id = $target.target_id; target_root = $target.target_root }
$deploy = Invoke-GenieAction -Id 'runtime.usb_genie.deploy' -Payload ($identity + @{ credential_ref = 'none'; zip_path = $ZipPath }) -Write $true
if (-not $deploy.sha256_ok) { throw 'deploy did not verify the pinned archive hash' }
$verify = Invoke-GenieAction -Id 'runtime.usb_genie.verify' -Payload $identity
if (-not $verify.ok) { throw ("verify failed: " + ($verify.blockers -join '; ')) }
if (Test-Path -LiteralPath (Join-Path $TargetRoot 'U-King\AI-Genie\data\.security.yml')) { throw 'credential-free smoke output leaked .security.yml' }
if ($Launch) { [void](Invoke-GenieAction -Id 'runtime.usb_genie.launch' -Payload $identity -Write $true) }
Write-Host "USB AI Genie smoke passed: $TargetRoot"
