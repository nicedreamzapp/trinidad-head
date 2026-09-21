# Drives a Trinidad Head window with window messages only (no real mouse/keyboard) and checks results.
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class TW {
 [StructLayout(LayoutKind.Sequential)] public struct R { public int L,T,Ri,B; }
 [StructLayout(LayoutKind.Sequential)] public struct P { public int X,Y; }
 [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string t);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out R r);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref P p);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
 public static IntPtr L(int x, int y) { return (IntPtr)((y << 16) | (x & 0xFFFF)); }
}
"@
$results = New-Object System.Collections.Generic.List[string]
function Check($name, $ok, $detail) { $results.Add(("{0} {1} {2}" -f ($(if ($ok) {"PASS"} else {"FAIL"})), $name, $detail)) }

$saved = [System.Windows.Forms.Clipboard]::GetText()
$exe = "$env:LOCALAPPDATA\Programs\TrinidadHead\trinidad-head.exe"
$py = "C:\Users\matt\AppData\Local\Programs\Python\Python311\python.exe"
Remove-Item C:\Users\matt\dev\th_test_input.log, C:\Users\matt\dev\th_test_dump.txt -ErrorAction SilentlyContinue
$env:TRINIDAD_HEAD_DUMP = "C:\Users\matt\dev\th_test_dump.txt"
$env:TRINIDAD_HEAD_SELFTEST = "1"
$menuFile = "C:\Users\matt\dev\th_test_dump.menu"; $linkFile = "C:\Users\matt\dev\th_test_dump.link"
Remove-Item $menuFile, $linkFile -ErrorAction SilentlyContinue
$WM_CANCELMODE = 0x1F
$settings = "$env:LOCALAPPDATA\TrinidadHead\settings.txt"
$glowBefore = if (Test-Path $settings) { Get-Content $settings -Raw } else { "" }
$regDir = "$env:LOCALAPPDATA\TrinidadHead\windows"

# Every test window is parked off-screen at x=-5000, so a run that dies partway leaves an
# invisible Trinidad Head running on the PC and Matt's clipboard full of test text. Track what
# this run starts and put the tidy-up in finally, which runs however the script ends: the runs
# that strand a window are exactly the ones that never reach the bottom of the file.
$spawned = New-Object System.Collections.Generic.List[object]
function StartWin($child) {
  $proc = Start-Process -FilePath $exe -ArgumentList "`"$py`" $child" -PassThru
  $spawned.Add($proc)
  return $proc
}
$WM_CLOSE = 0x0010

# Ends every window this run started that is still up. Close, do not kill: the window hangs up
# its shell and drops its taskbar button on its own, and a killed one leaves both behind.
function CloseSpawned($windows, $log) {
  $stranded = 0
  foreach ($proc in $windows) {
    try { $proc.Refresh() } catch { }
    if (-not $proc.HasExited) {
      $stranded++
      try { [void][TW]::PostMessage($proc.MainWindowHandle, 0x0010, [IntPtr]0, [IntPtr]0) } catch { }
      if (-not $proc.WaitForExit(8000)) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    }
  }
  if ($stranded -gt 0) { $log.Add("NOTE closed $stranded test window(s) this run left open") }
  return $stranded
}
# TEST-CUT (pc-tools/th_cleanup_test.ps1 reads everything above this line)
try {
$p = StartWin "C:\Users\matt\dev\pctools\th_child.py"
Start-Sleep 3
$p.Refresh(); $hw = $p.MainWindowHandle
# Park the test window off to the side so Matt's real mouse can't land in it mid-test.
$wr0 = New-Object TW+R; [void][TW]::GetWindowRect($hw, [ref]$wr0)
[void][TW]::SetWindowPos($hw, [IntPtr]::Zero, -5000, 200, 0, 0, 0x15)
Start-Sleep -m 500
$s = [TW]::GetDpiForWindow($hw) / 96.0
$r = New-Object TW+R; [void][TW]::GetClientRect($hw, [ref]$r)
# Layout (matches layout.rs): margin MARGIN (26), floor FLOOR (40), text starts at sidebar.r + 18 and body.t + 56.
$M = 26; $F = 40
$tl = [int](($M + 16 + 40 + 18) * $s); $tt = [int](($M + 56) * $s)
$ch = 15 * 1.5 * $s   # approximate row height, only used to aim at a row
$WM_LD=0x201; $WM_LU=0x202; $WM_MV=0x200; $WM_RU=0x205; $WM_KD=0x100; $WM_CH=0x102; $WM_WH=0x20A

# Phase A: mouse reporting on.
$x = $tl + 40; $y = $tt + 20
[void][TW]::PostMessage($hw, $WM_LD, [IntPtr]1, [TW]::L($x, $y)); Start-Sleep -m 150
[void][TW]::PostMessage($hw, $WM_LU, [IntPtr]0, [TW]::L($x, $y)); Start-Sleep -m 150
[void][TW]::PostMessage($hw, $WM_LD, [IntPtr]1, [TW]::L($x, $y)); Start-Sleep -m 150
[void][TW]::PostMessage($hw, $WM_MV, [IntPtr]1, [TW]::L($x + 60, $y)); Start-Sleep -m 150
[void][TW]::PostMessage($hw, $WM_LU, [IntPtr]0, [TW]::L($x + 60, $y)); Start-Sleep -m 150
[System.Windows.Forms.Clipboard]::SetText("paste-ok")
[void][TW]::PostMessage($hw, $WM_RU, [IntPtr]0, [TW]::L($x, $y)); Start-Sleep -m 800
$menuOpen = [TW]::FindWindow("#32768", $null) -ne [IntPtr]::Zero
$menuA = if (Test-Path $menuFile) { (Get-Content $menuFile) -join "," } else { "" }
[void][TW]::PostMessage($hw, $WM_CANCELMODE, [IntPtr]0, [IntPtr]0); Start-Sleep -m 600
$menuClosed = [TW]::FindWindow("#32768", $null) -eq [IntPtr]::Zero
Check "right-click opens a menu" $menuOpen ""
Check "menu: Copy on after drag-select, Paste on, Select All" ($menuA -eq "Copy=1,Paste=1,Select All=1") $menuA
Check "menu closes" $menuClosed ""
$pt = New-Object TW+P; $pt.X = $x; $pt.Y = $y; [void][TW]::ClientToScreen($hw, [ref]$pt)
[void][TW]::PostMessage($hw, $WM_WH, [IntPtr](120 -shl 16), [TW]::L($pt.X, $pt.Y)); Start-Sleep -m 150
[void][TW]::PostMessage($hw, $WM_KD, [IntPtr]0x26, [IntPtr]0); Start-Sleep -m 150   # VK_UP
[void][TW]::PostMessage($hw, $WM_CH, [IntPtr][int][char]'x', [IntPtr]0); Start-Sleep -m 150
[System.Windows.Forms.Clipboard]::SetText("paste-ok")
# Real drag and drop of a file onto the (visible) window.
$dropFile = "C:\Users\matt\dev\th drop test.png"; Set-Content $dropFile "x"
$wr1 = New-Object TW+R; [void][TW]::GetWindowRect($hw, [ref]$wr1)
[void][TW]::SetWindowPos($hw, [IntPtr](-1), 300, 150, 0, 0, 0x11); Start-Sleep -m 600
$tp = New-Object TW+P; $tp.X = $tl + 200; $tp.Y = $tt + 120; [void][TW]::ClientToScreen($hw, [ref]$tp)
$dragOut = & powershell -NoProfile -STA -ExecutionPolicy Bypass -File C:\Users\matt\dev\pctools\th_drag.ps1 -File $dropFile -SX 150 -SY 150 -TX $tp.X -TY $tp.Y
[void][TW]::SetWindowPos($hw, [IntPtr](-2), -5000, 200, 0, 0, 0x11); Start-Sleep -m 600
$log = if (Test-Path C:\Users\matt\dev\th_test_input.log) { (Get-Content C:\Users\matt\dev\th_test_input.log) -replace "^b'", "" -replace "'$", "" -join "" } else { "" }
Check "mouse press/release reported" ($log -match '\[<0;\d+;\d+M' -and $log -match '\[<0;\d+;\d+m') ""
Check "wheel reported" ($log -match '\[<64;\d+;\d+M') ""
Check "arrow key" ($log -match '\\x1b\[A') ""
Check "typed character" ((Get-Content C:\Users\matt\dev\th_test_input.log) -contains "b'x'") ""
Check "click (no drag) reported once" (([regex]::Matches($log, '\[<0;\d+;\d+M')).Count -eq 1) ""
Check "drag under mouse mode is not sent" (-not ($log -match '\[<32;')) ""
Check "right-click does not paste" (-not ($log -match 'paste-ok')) ""
Check "real file drop typed as quoted path" ($log -match [regex]::Escape('200~"C:\\Users\\matt\\dev\\th drop test.png" \x1b[201~')) "$dragOut"

# Phase B: mouse off, OSC 52, then a local selection. Wait for the child to print it.
# The dump is written at most 4 times a second, on repaint; ask for repaints while waiting.
for ($w = 0; $w -lt 40; $w++) { [void][TW]::PostMessage($hw, 0x000F, [IntPtr]0, [IntPtr]0); Start-Sleep -m 300; if ((Get-Content C:\Users\matt\dev\th_test_dump.txt -ErrorAction SilentlyContinue) -like "SELECT-ME*") { break }; Start-Sleep -m 500 }
Start-Sleep 1
Check "OSC 52 copy" ([System.Windows.Forms.Clipboard]::GetText() -eq "osc52-ok") ([System.Windows.Forms.Clipboard]::GetText())
# The dump is rewritten a few times a second; retry if we catch it mid-write.
$row = -1
for ($k = 0; $k -lt 20 -and $row -lt 0; $k++) {
  $dump = Get-Content C:\Users\matt\dev\th_test_dump.txt -ErrorAction SilentlyContinue
  for ($i = 0; $i -lt $dump.Count; $i++) { if ($dump[$i] -like "SELECT-ME*") { $row = $i; break } }
  if ($row -lt 0) { [void][TW]::PostMessage($hw, 0x000F, [IntPtr]0, [IntPtr]0); Start-Sleep -m 300 }
}
$cellH = ($r.B - ($M + $F) * $s - 56 * $s - 28 * $s) / ([int]([regex]::Match(($dump | Select-String "size").Line, 'x(\d+)').Groups[1].Value))
$sy = [int]($tt + ($row + 0.5) * $cellH)
[void][TW]::PostMessage($hw, $WM_LD, [IntPtr]1, [TW]::L($tl + 2, $sy)); Start-Sleep -m 120
[void][TW]::PostMessage($hw, $WM_MV, [IntPtr]1, [TW]::L($tl + 300 * $s, $sy)); Start-Sleep -m 120
[void][TW]::PostMessage($hw, $WM_LU, [IntPtr]0, [TW]::L($tl + 300 * $s, $sy)); Start-Sleep -m 400
Check "selecting alone does not copy" ([System.Windows.Forms.Clipboard]::GetText() -eq "osc52-ok") ""
# Right-click, then Down + Enter picks Copy (first item).
[void][TW]::PostMessage($hw, $WM_RU, [IntPtr]0, [TW]::L($tl + 2, $sy)); Start-Sleep -m 700
[void][TW]::PostMessage($hw, $WM_KD, [IntPtr]0x28, [IntPtr]0); Start-Sleep -m 200
[void][TW]::PostMessage($hw, $WM_KD, [IntPtr]0x0D, [IntPtr]0); Start-Sleep -m 600
if ([TW]::FindWindow("#32768", $null) -ne [IntPtr]::Zero) { [void][TW]::PostMessage($hw, $WM_CANCELMODE, [IntPtr]0, [IntPtr]0); Start-Sleep -m 400 }
$clip = [System.Windows.Forms.Clipboard]::GetText()
Check "menu Copy copies the selection" ($clip -like "SELECT-ME*") ($clip -replace "`r?`n", "|")
# Links: right-click over the link offers Open/Copy Link; Ctrl+click opens it.
$ly2 = [int]($sy + $cellH); $lx2 = [int]($tl + 12 * $cellH * 0.45)
[void][TW]::PostMessage($hw, $WM_RU, [IntPtr]0, [TW]::L($lx2, $ly2)); Start-Sleep -m 700
$menuL = if (Test-Path $menuFile) { (Get-Content $menuFile) -join "," } else { "" }
[void][TW]::PostMessage($hw, $WM_CANCELMODE, [IntPtr]0, [IntPtr]0); Start-Sleep -m 400
Check "link menu" ($menuL -like "Open Link=1,Copy Link=1,*") $menuL
[void][TW]::PostMessage($hw, $WM_LD, [IntPtr]9, [TW]::L($lx2, $ly2)); Start-Sleep -m 100
[void][TW]::PostMessage($hw, $WM_LU, [IntPtr]8, [TW]::L($lx2, $ly2)); Start-Sleep -m 400
$opened = if (Test-Path $linkFile) { Get-Content $linkFile -Raw } else { "" }
Check "Ctrl+click opens the link" ($opened -eq "https://ineedhemp.com/shop") $opened

# Drag-autoscroll: a drag held past the top edge keeps scrolling on its own, so the copy can
# run past what the window is showing without resizing it. Wait for the child's numbered block.
$adump = @()
for ($w = 0; $w -lt 60; $w++) {
  [void][TW]::PostMessage($hw, 0x000F, [IntPtr]0, [IntPtr]0); Start-Sleep -m 400
  $adump = Get-Content C:\Users\matt\dev\th_test_dump.txt -ErrorAction SilentlyContinue
  if (($adump -join "`n") -match "AUTOSCROLL-LINE-120") { break }
}
$rows = [int]([regex]::Match((($adump | Select-String "size").Line), 'x(\d+)').Groups[1].Value)
$firstLine = 0; $lastRow = -1
for ($i = 0; $i -lt $rows -and $i -lt $adump.Count; $i++) {
  if ($adump[$i] -match '^AUTOSCROLL-LINE-(\d+)') {
    if ($firstLine -eq 0) { $firstLine = [int]$Matches[1] }
    $lastRow = $i
  }
}
$ax = $tl + 2
$ay = [int]($tt + ($lastRow + 0.5) * $cellH)
# Press on the last numbered row, then hold the pointer above the text and stop moving it:
# nothing but the autoscroll can grow this selection now.
[void][TW]::PostMessage($hw, $WM_LD, [IntPtr]1, [TW]::L($ax, $ay)); Start-Sleep -m 150
[void][TW]::PostMessage($hw, $WM_MV, [IntPtr]1, [TW]::L($ax, $ay - 10)); Start-Sleep -m 150
[void][TW]::PostMessage($hw, $WM_MV, [IntPtr]1, [TW]::L($ax, $tt - 20)); Start-Sleep -m 1500
[void][TW]::PostMessage($hw, $WM_LU, [IntPtr]0, [TW]::L($ax, $tt - 20)); Start-Sleep -m 400
[System.Windows.Forms.Clipboard]::SetText("before-autoscroll")
[void][TW]::PostMessage($hw, $WM_RU, [IntPtr]0, [TW]::L($ax, $ay)); Start-Sleep -m 700
[void][TW]::PostMessage($hw, $WM_KD, [IntPtr]0x28, [IntPtr]0); Start-Sleep -m 200
[void][TW]::PostMessage($hw, $WM_KD, [IntPtr]0x0D, [IntPtr]0); Start-Sleep -m 600
if ([TW]::FindWindow("#32768", $null) -ne [IntPtr]::Zero) { [void][TW]::PostMessage($hw, $WM_CANCELMODE, [IntPtr]0, [IntPtr]0); Start-Sleep -m 400 }
$diag = ""
$d2 = Get-Content C:\Users\matt\dev\th_test_dump.txt -ErrorAction SilentlyContinue
foreach ($ln in $d2) { if ($ln -like "scroll * autoscroll *") { $diag = $ln } }
$grab = [System.Windows.Forms.Clipboard]::GetText()
$grabbed = ($grab -split "`r?`n").Count
Check "a drag held above the top edge keeps scrolling" ($grabbed -gt $rows) "copied $grabbed lines; the screen holds $rows; $diag"
$topLine = 0
if ($grab -match '^AUTOSCROLL-LINE-(\d+)') { $topLine = [int]$Matches[1] }
Check "the copy starts above the first line that was on screen" (($topLine -gt 0) -and ($topLine -lt $firstLine)) "copy starts at line $topLine; the screen started at line $firstLine"
# Back down to the live screen before the rest of the checks.
for ($i = 0; $i -lt 60; $i++) { [void][TW]::PostMessage($hw, $WM_WH, [IntPtr](-120 -shl 16), [TW]::L($ax, $ay)) }
Start-Sleep -m 500

# Selecting past the edge inside a program that owns the screen, the way Claude Code does.
# The child takes the alternate screen, keeps 300 lines to itself and scrolls on wheel reports,
# so none of that text is in our scrollback and grid coordinates cannot express the selection.
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep 1
$q = StartWin "C:\Users\matt\dev\pctools\th_fullscreen_child.py"
Start-Sleep 3
$q.Refresh(); $qh = $q.MainWindowHandle
[void][TW]::SetWindowPos($qh, [IntPtr]::Zero, -5000, 200, 0, 0, 0x15)
Start-Sleep -m 800
$qd = ""
for ($w = 0; $w -lt 40; $w++) {
  [void][TW]::PostMessage($qh, 0x000F, [IntPtr]0, [IntPtr]0); Start-Sleep -m 400
  $qd = Get-Content C:\Users\matt\dev\th_test_dump.txt -ErrorAction SilentlyContinue
  if (($qd -join "`n") -match "PROGRAM-LINE-") { break }
}
$qrows = [int]([regex]::Match((($qd | Select-String "size").Line), 'x(\d+)').Groups[1].Value)
$qfirst = 0
foreach ($ln in $qd) { if ($ln -match '^PROGRAM-LINE-(\d+)') { $qfirst = [int]$Matches[1]; break } }
$qlast = -1
for ($i = 0; $i -lt $qrows -and $i -lt $qd.Count; $i++) { if ($qd[$i] -match '^PROGRAM-LINE-\d+') { $qlast = $i } }
$qy = [int]($tt + ($qlast + 0.5) * $cellH)
[System.Windows.Forms.Clipboard]::SetText("before-program")
[void][TW]::PostMessage($qh, $WM_LD, [IntPtr]1, [TW]::L($tl + 2, $qy)); Start-Sleep -m 150
[void][TW]::PostMessage($qh, $WM_MV, [IntPtr]1, [TW]::L($tl + 2, $qy - 10)); Start-Sleep -m 150
[void][TW]::PostMessage($qh, $WM_MV, [IntPtr]1, [TW]::L($tl + 2, $tt - 20)); Start-Sleep -m 3000
[void][TW]::PostMessage($qh, $WM_LU, [IntPtr]0, [TW]::L($tl + 2, $tt - 20)); Start-Sleep -m 500
[void][TW]::PostMessage($qh, $WM_RU, [IntPtr]0, [TW]::L($tl + 2, $qy)); Start-Sleep -m 700
[void][TW]::PostMessage($qh, $WM_KD, [IntPtr]0x28, [IntPtr]0); Start-Sleep -m 200
[void][TW]::PostMessage($qh, $WM_KD, [IntPtr]0x0D, [IntPtr]0); Start-Sleep -m 700
if ([TW]::FindWindow("#32768", $null) -ne [IntPtr]::Zero) { [void][TW]::PostMessage($qh, $WM_CANCELMODE, [IntPtr]0, [IntPtr]0); Start-Sleep -m 400 }
$pg = [System.Windows.Forms.Clipboard]::GetText()
$pgl = @($pg -split "`r?`n" | Where-Object { $_ -match '^PROGRAM-LINE-\d+$' })
Check "full-screen program: the copy holds more than it was showing" ($pgl.Count -gt $qrows) "copied $($pgl.Count) lines; the screen holds $qrows"
$pgn = @($pgl | ForEach-Object { [int]($_ -replace '\D','') })
$ordered = $true
for ($i = 1; $i -lt $pgn.Count; $i++) { if ($pgn[$i] -ne $pgn[$i-1] + 1) { $ordered = $false; break } }
Check "full-screen program: every line in order, none repeated or skipped" ($ordered -and $pgn.Count -gt 0) "$($pgn[0])..$($pgn[-1]) of $($pgn.Count)"
Check "full-screen program: the copy reaches past where the screen started" (($pgn.Count -gt 0) -and ($pgn[0] -lt $qfirst)) "copy starts at $($pgn[0]); the screen started at $qfirst"
Stop-Process -Id $q.Id -Force -ErrorAction SilentlyContinue
Start-Sleep 1
$p = StartWin "C:\Users\matt\dev\pctools\th_child.py"
Start-Sleep 3
$p.Refresh(); $hw = $p.MainWindowHandle
[void][TW]::SetWindowPos($hw, [IntPtr]::Zero, -5000, 200, 0, 0, 0x15)
Start-Sleep -m 500

# Resize.
$before = ($dump | Select-String "size").Line
$wr = New-Object TW+R; [void][TW]::GetWindowRect($hw, [ref]$wr)
[void][TW]::SetWindowPos($hw, [IntPtr]::Zero, $wr.L, $wr.T, 900, 560, 0x14); Start-Sleep 1.5
$after = (Get-Content C:\Users\matt\dev\th_test_dump.txt | Select-String "size").Line
Check "resize changes terminal size" ($before -ne $after) "$before -> $after"

# Buttons (light centres from layout.rs: pill at body.l+58, y body.t+14, lights at +16/+37/+58, centre y +13).
[void][TW]::GetClientRect($hw, [ref]$r)
$ly = [int](($M + 14 + 13) * $s)
function Click($cx) { [void][TW]::PostMessage($hw, $WM_LD, [IntPtr]1, [TW]::L($cx, $ly)); Start-Sleep -m 100; [void][TW]::PostMessage($hw, $WM_LU, [IntPtr]0, [TW]::L($cx, $ly)); Start-Sleep -m 700 }
Click ([int](($M + 58 + 58) * $s)); Check "green zooms" ([TW]::IsZoomed($hw)) ""
# Zoomed: no margin, so the lights move; restore directly.
[void][TW]::ShowWindow($hw, 9); Start-Sleep -m 700
Check "restore from zoom" (-not [TW]::IsZoomed($hw)) ""
Click ([int](($M + 58 + 37) * $s)); Check "yellow minimizes" ([TW]::IsIconic($hw)) ""
[void][TW]::ShowWindow($hw, 9); Start-Sleep -m 700
# Glow button: the only sidebar slot (x = body.l+16+20, y = sidebar top + 6 + 20).
$glowReg = Get-Content (Join-Path $regDir $p.Id) -Raw
[void][TW]::GetClientRect($hw, [ref]$r)
$bodyH = $r.B - ($M + $F) * $s
$sideH = [Math]::Max(40 * $s, [Math]::Min(52 * $s, $bodyH - 90 * $s))
$sideT = $M * $s + $bodyH / 2 - $sideH / 2 + 14 * $s
$gx = [int](($M + 16 + 20) * $s); $gy = [int]($sideT + (6 + 20) * $s)
[void][TW]::PostMessage($hw, $WM_LD, [IntPtr]1, [TW]::L($gx, $gy)); Start-Sleep -m 100; [void][TW]::PostMessage($hw, $WM_LU, [IntPtr]0, [TW]::L($gx, $gy)); Start-Sleep -m 500
$regFile = Join-Path $regDir $p.Id
$glowAfter = Get-Content $regFile -Raw
Check "glow button changes this window's color" ($glowAfter -ne $glowReg) "$glowReg -> $glowAfter"
for ($i = 0; $i -lt 3; $i++) { [void][TW]::PostMessage($hw, $WM_LD, [IntPtr]1, [TW]::L($gx, $gy)); Start-Sleep -m 80; [void][TW]::PostMessage($hw, $WM_LU, [IntPtr]0, [TW]::L($gx, $gy)); Start-Sleep -m 300 }
$glowBack = if (Test-Path $settings) { Get-Content $settings -Raw } else { "" }
Check "default glow setting untouched" ($glowBack -eq $glowBefore) ""
# Red closes.
Click ([int](($M + 58 + 16) * $s)); Start-Sleep 1
Check "red closes the window" ($p.HasExited) ""
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
} finally {
  [System.Windows.Forms.Clipboard]::SetText($(if ($saved) { $saved } else { " " }))
  [void](CloseSpawned $spawned $results)
}
$results
