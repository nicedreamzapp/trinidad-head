# Trinidad Head - Windows uninstaller.
#
#   irm https://raw.githubusercontent.com/nicedreamzapp/trinidad-head/main/uninstall.ps1 | iex
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 [-Purge]
#
# Removes trinidad-head.exe and its Start Menu / Desktop shortcuts. Your settings in
# %LOCALAPPDATA%\TrinidadHead are kept unless you pass -Purge (or set TH_PURGE=1).
# Close Trinidad Head first; a running copy can't be deleted (it is left and reported).
# Same settings as install.ps1: -Dir/TH_DIR, -ShortcutDir/TH_SHORTCUT_DIR.

function Uninstall-TrinidadHead {
  param(
    [string]$Dir = $(if ($env:TH_DIR) { $env:TH_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\TrinidadHead" }),
    [string]$ShortcutDir = $(if ($env:TH_SHORTCUT_DIR) { $env:TH_SHORTCUT_DIR } else { [Environment]::GetFolderPath("Programs") }),
    [string]$DataDir = $(if ($env:TH_DATA_DIR) { $env:TH_DATA_DIR } else { Join-Path $env:LOCALAPPDATA "TrinidadHead" }),
    [switch]$Purge = ($env:TH_PURGE -eq "1")
  )
  $ErrorActionPreference = "Continue"
  Write-Host "== Trinidad Head - uninstall =="
  # Only shortcuts that point at THIS install's exe; anything else is left alone.
  $exe = Join-Path $Dir "trinidad-head.exe"
  $ws = New-Object -ComObject WScript.Shell
  foreach ($d in @($ShortcutDir, [Environment]::GetFolderPath("Desktop"))) {
    $lnk = Join-Path $d "Trinidad Head.lnk"
    if (-not (Test-Path $lnk)) { continue }
    if ($ws.CreateShortcut($lnk).TargetPath -ieq $exe) { Remove-Item $lnk -Force; Write-Host "-> Removed $lnk" }
    else { Write-Host "-> Left $lnk (it points somewhere else)" }
  }
  if (Test-Path $Dir) {
    Remove-Item $Dir -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path $Dir) { Write-Host "-> Could not delete all of $Dir (is Trinidad Head still open?)" }
    else { Write-Host "-> Deleted $Dir" }
  } else { Write-Host "-> $Dir not found" }
  if ($Purge) {
    if (Test-Path $DataDir) { Remove-Item $DataDir -Recurse -Force; Write-Host "-> Deleted settings $DataDir" }
  } elseif (Test-Path $DataDir) {
    Write-Host "-> Kept settings in $DataDir (run with -Purge to delete them)"
  }
  Write-Host "Done."
}

Uninstall-TrinidadHead @args
