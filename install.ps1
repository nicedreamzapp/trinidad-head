# Trinidad Head - Windows installer.
#
#   irm https://raw.githubusercontent.com/nicedreamzapp/trinidad-head/main/install.ps1 | iex
#
# Downloads Trinidad-Head-windows.zip from the latest GitHub release (checked against the SHA-256
# GitHub publishes for it), puts trinidad-head.exe in %LOCALAPPDATA%\Programs\TrinidadHead and adds
# a "Trinidad Head" shortcut to the Start Menu. A copy that is running is renamed aside, not
# overwritten, so you can install while it is open. Nothing needs admin.
#
# Settings (params when run from a file, env vars when piped into iex):
#   -Dir         / TH_DIR           install folder          (default %LOCALAPPDATA%\Programs\TrinidadHead)
#   -Version     / TH_VERSION       release tag, e.g. v0.2.0 (default latest)
#   -ShortcutDir / TH_SHORTCUT_DIR  where the shortcut goes (default your Start Menu Programs folder)
#   -Desktop                        also put a shortcut on the Desktop

function Install-TrinidadHead {
  param(
    [string]$Dir = $(if ($env:TH_DIR) { $env:TH_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\TrinidadHead" }),
    [string]$Version = $env:TH_VERSION,
    [string]$ShortcutDir = $(if ($env:TH_SHORTCUT_DIR) { $env:TH_SHORTCUT_DIR } else { [Environment]::GetFolderPath("Programs") }),
    [switch]$Desktop
  )
  $ErrorActionPreference = "Stop"
  $ProgressPreference = "SilentlyContinue"   # Invoke-WebRequest is many times faster without the progress bar
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

  Write-Host ""
  Write-Host "== Trinidad Head - Windows installer =="
  $api = "https://api.github.com/repos/nicedreamzapp/trinidad-head/releases/" + $(if ($Version) { "tags/$Version" } else { "latest" })
  $rel = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "trinidad-head-installer"; "Accept" = "application/vnd.github+json" }
  $asset = $rel.assets | Where-Object { $_.name -like "*windows*.zip" } | Select-Object -First 1
  if (-not $asset) { throw "release $($rel.tag_name) has no Windows zip" }
  Write-Host "-> $($rel.tag_name): $($asset.name) ($([math]::Round($asset.size / 1KB)) KB)"

  $tmp = Join-Path ([IO.Path]::GetTempPath()) ("th-install-" + [guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Force $tmp | Out-Null
  try {
    $zip = Join-Path $tmp $asset.name
    Invoke-WebRequest -UseBasicParsing -Uri $asset.browser_download_url -OutFile $zip -Headers @{ "User-Agent" = "trinidad-head-installer" }
    if ((Get-Item $zip).Length -ne $asset.size) { throw "download is $((Get-Item $zip).Length) bytes, expected $($asset.size)" }
    if ($asset.digest -and $asset.digest -like "sha256:*") {
      $want = $asset.digest.Substring(7).ToLower()
      $got = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
      if ($got -ne $want) { throw "checksum mismatch: got $got, expected $want" }
      Write-Host "-> SHA-256 verified"
    }
    $x = Join-Path $tmp "x"
    Expand-Archive -Path $zip -DestinationPath $x -Force
    $src = Get-ChildItem $x -Recurse -Filter *.exe | Select-Object -First 1
    if (-not $src) { throw "no .exe inside $($asset.name)" }

    New-Item -ItemType Directory -Force $Dir | Out-Null
    $exe = Join-Path $Dir "trinidad-head.exe"
    if (Test-Path $exe) { Rename-Item $exe ("trinidad-head.old-" + (Get-Date -Format yyyyMMddHHmmss) + ".exe") }
    Copy-Item $src.FullName $exe
    Get-ChildItem $Dir -Filter "trinidad-head.old-*.exe" | ForEach-Object { try { Remove-Item $_.FullName -ErrorAction Stop } catch { } }
    if ((Get-Item $exe).Length -ne $src.Length) { throw "copy did not take" }
    Write-Host "-> Installed $exe"
  } finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
  }

  $ws = New-Object -ComObject WScript.Shell
  $places = @($ShortcutDir)
  if ($Desktop) { $places += [Environment]::GetFolderPath("Desktop") }
  foreach ($d in $places) {
    New-Item -ItemType Directory -Force $d | Out-Null
    $s = $ws.CreateShortcut((Join-Path $d "Trinidad Head.lnk"))
    $s.TargetPath = $exe; $s.WorkingDirectory = $env:USERPROFILE; $s.IconLocation = "$exe,0"; $s.Description = "Trinidad Head terminal"
    $s.Save()
    Write-Host "-> Shortcut: $(Join-Path $d 'Trinidad Head.lnk')"
  }
  Write-Host ""
  Write-Host "Done. Open Trinidad Head from the Start Menu."
  Write-Host "Uninstall: irm https://raw.githubusercontent.com/nicedreamzapp/trinidad-head/main/uninstall.ps1 | iex"
}

Install-TrinidadHead @args
