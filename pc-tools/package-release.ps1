# Build the Windows download: trinidad-head.exe zipped for a GitHub release.
#   powershell -NoProfile -ExecutionPolicy Bypass -File pc-tools\package-release.ps1
# -> dist\Trinidad-Head-windows.zip (one exe, no installer, nothing else needed to run it)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$dist = Join-Path $root "dist"
$stage = Join-Path $dist "windows"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "target\release\trinidad-head.exe" (Join-Path $stage "Trinidad Head.exe")
$zip = Join-Path $dist "Trinidad-Head-windows.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
Get-Item $zip | Select-Object Name, Length
