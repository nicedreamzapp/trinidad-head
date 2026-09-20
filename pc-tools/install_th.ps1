# Copies the freshly built Trinidad Head into its install folder. A running copy is renamed
# out of the way first (Windows allows renaming, not overwriting, an exe that's in use).
#
# The source is the git repo. It used to be C:\Users\matt\dev\our-terminal, the pre-git build
# folder, which stopped being rebuilt on 2026-09-17 — every "install" after that quietly put
# the same stale exe back, so nothing shipped to the PC for three days.
$src = "C:\Users\matt\dev\trinidad-head\target\release\trinidad-head.exe"
if (-not (Test-Path $src)) { Write-Error "no build at $src - run cargo build --release first"; exit 1 }
$dst = "$env:LOCALAPPDATA\Programs\TrinidadHead"
New-Item -ItemType Directory -Force $dst | Out-Null
$exe = "$dst\trinidad-head.exe"
if (Test-Path $exe) { Rename-Item $exe ("trinidad-head.old-" + (Get-Date -Format yyyyMMddHHmmss) + ".exe") }
Copy-Item $src $exe
Get-ChildItem $dst -Filter "trinidad-head.old-*.exe" | ForEach-Object { try { Remove-Item $_.FullName -ErrorAction Stop } catch {} }
$built = (Get-Item $src).LastWriteTime
"installed $exe $((Get-Item $exe).Length) bytes, built $built"
if ((Get-Item $exe).Length -ne (Get-Item $src).Length) { Write-Error "copy did not take" }
