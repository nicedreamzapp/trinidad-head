# Deleting a highlight inside a program's own input box (Claude Code's prompt), on the PC.
# Drives one Trinidad Head window parked off-screen with window messages only, against
# scripts\prompt-child.py: a stand-in for Claude's prompt that writes its exact text to a file,
# so every check reads the program's own buffer. Same checks as the Mac `prompt-edit` mode.
# Run from the repo clone:  powershell -NoProfile -ExecutionPolicy Bypass -File pc-tools\th_prompt_test.ps1
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class TP {
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
 public static IntPtr L(int x, int y) { return (IntPtr)((y << 16) | (x & 0xFFFF)); }
}
"@
$results = New-Object System.Collections.Generic.List[string]
function Check($name, $ok, $detail) { $results.Add(("{0} {1} {2}" -f ($(if ($ok) {"PASS"} else {"FAIL"})), $name, $detail)) }

$repo = Split-Path -Parent $PSScriptRoot
$exe = "$env:LOCALAPPDATA\Programs\TrinidadHead\trinidad-head.exe"
$py = "C:\Users\matt\AppData\Local\Programs\Python\Python311\python.exe"
$out = "C:\Users\matt\dev\th_prompt_out.txt"
$dumpFile = "C:\Users\matt\dev\th_prompt_dump.txt"
Remove-Item $out, $dumpFile -ErrorAction SilentlyContinue
$env:PROMPT_CHILD_OUT = $out
$env:TRINIDAD_HEAD_DUMP = $dumpFile
$env:TRINIDAD_HEAD_SELFTEST = "1"
$WM_LD=0x201; $WM_LU=0x202; $WM_MV=0x200; $WM_KD=0x100; $WM_KU=0x101; $WM_CH=0x102; $WM_PAINT=0x000F

$p = Start-Process -FilePath $exe -ArgumentList "`"$py`" `"$repo\scripts\prompt-child.py`"" -PassThru
try {
Start-Sleep 3
$p.Refresh(); $hw = $p.MainWindowHandle
# Parked off to the side, so Matt's real mouse can't land in it mid-test.
[void][TP]::SetWindowPos($hw, [IntPtr]::Zero, -5000, 200, 0, 0, 0x15)
Start-Sleep -m 500

function Dump {
  for ($k = 0; $k -lt 10; $k++) {
    [void][TP]::PostMessage($hw, $WM_PAINT, [IntPtr]0, [IntPtr]0); Start-Sleep -m 300
    $d = Get-Content $dumpFile -Encoding UTF8 -ErrorAction SilentlyContinue
    if ($d -and ($d -join "`n") -match "text [\d\.-]+,") { return ,$d }
  }
  return ,@()
}
function Buf { if (Test-Path $out) { [System.IO.File]::ReadAllText($out) } else { "<none>" } }
function WaitBuf($want) {
  for ($k = 0; $k -lt 40; $k++) { if ((Buf) -ceq $want) { return $true }; Start-Sleep -m 100 }
  return $false
}
# Screen geometry from the dump: grid size and the text rectangle in client pixels.
function Geo {
  $d = Dump
  $size = [regex]::Match(($d -join "`n"), 'size (\d+)x(\d+)')
  $rect = [regex]::Match(($d -join "`n"), 'text ([\d\.-]+),([\d\.-]+),([\d\.-]+),([\d\.-]+)')
  $cols = [int]$size.Groups[1].Value; $rows = [int]$size.Groups[2].Value
  $l = [double]$rect.Groups[1].Value; $t = [double]$rect.Groups[2].Value
  $r = [double]$rect.Groups[3].Value; $b = [double]$rect.Groups[4].Value
  # The window's real cell size: the text rectangle is rarely a whole number of cells.
  $cell = [regex]::Match(($d -join "`n"), 'cell ([\d\.]+)x([\d\.]+)')
  return @{ d = $d; cols = $cols; rows = $rows; l = $l; t = $t; cw = [double]$cell.Groups[1].Value; ch = [double]$cell.Groups[2].Value }
}
function Find($g, $word) {
  for ($i = 0; $i -lt $g.rows -and $i -lt $g.d.Count; $i++) {
    $c = $g.d[$i].IndexOf($word)
    if ($c -ge 0) { return @($i, $c, ($c + $word.Length - 1)) }
  }
  return $null
}
function Px($g, $row, $col) { return @([int]($g.l + ($col + 0.5) * $g.cw), [int]($g.t + ($row + 0.5) * $g.ch)) }
function Drag($g, $r1, $c1, $r2, $c2) {
  $a = Px $g $r1 $c1; $b = Px $g $r2 $c2
  [void][TP]::PostMessage($hw, $WM_LD, [IntPtr]1, [TP]::L($a[0], $a[1])); Start-Sleep -m 120
  [void][TP]::PostMessage($hw, $WM_MV, [IntPtr]1, [TP]::L([int](($a[0] + $b[0]) / 2), [int](($a[1] + $b[1]) / 2))); Start-Sleep -m 120
  [void][TP]::PostMessage($hw, $WM_MV, [IntPtr]1, [TP]::L($b[0], $b[1])); Start-Sleep -m 120
  [void][TP]::PostMessage($hw, $WM_LU, [IntPtr]0, [TP]::L($b[0], $b[1])); Start-Sleep -m 300
}
# The dump is only rewritten on a repaint, so wait until its prompt row shows what the
# program's buffer really holds before aiming at a word in it.
function FreshGeo {
  for ($k = 0; $k -lt 15; $k++) {
    $g = Geo; $want = Buf
    $row = $g.d | Where-Object { $_.Length -gt 1 -and $_[0] -eq [char]0x276F } | Select-Object -First 1
    if ($row -and $want.StartsWith($row.Substring(2).TrimEnd())) { return $g }
  }
  return $g
}
function HighlightWord($word) {
  $g = FreshGeo; $f = Find $g $word
  if ($f) { Drag $g $f[0] $f[1] $f[0] $f[2] } else { $results.Add("NOTE $word is not on screen: rows $($g.rows), lines $($g.d.Count): " + (($g.d | Where-Object { $_ -ne "" }) -join " | ")) }
}
function Char($c) { [void][TP]::PostMessage($hw, $WM_CH, [IntPtr][int][char]$c, [IntPtr]0); Start-Sleep -m 30 }
function Backspace { [void][TP]::PostMessage($hw, $WM_CH, [IntPtr]8, [IntPtr]0); Start-Sleep -m 50 }
# Typed a character at a time: Ctrl+Shift+V reads the real key state, which posted messages
# cannot fake. Typing goes through the same path a paste does.
function TypeText($t) { foreach ($ch in $t.ToCharArray()) { Char $ch } }
function Cuts { $d = Get-Content $dumpFile -Encoding UTF8 -ErrorAction SilentlyContinue; $m = [regex]::Match(($d -join "`n"), 'prompt cuts (\d+)'); if ($m.Success) { [int]$m.Groups[1].Value } else { -1 } }

for ($w = 0; $w -lt 30; $w++) { $g = Geo; if (($g.d -join "`n") -match [char]0x276F) { break } }
Check "the stand-in prompt painted" ((($g.d -join "`n") -match [char]0x276F)) (($g.d | Select-Object -Last 8) -join " | ")

TypeText "alpha bravo charlie delta echo"
Check "text typed into the prompt" (WaitBuf "alpha bravo charlie delta echo") (Buf)
$g = Geo
$results.Add("NOTE screen after typing: " + (($g.d | Where-Object { $_ -ne "" }) -join " | "))

Backspace
Check "Backspace with no highlight deletes one character" (WaitBuf "alpha bravo charlie delta ech") (Buf)

HighlightWord "bravo"; Backspace
Check "highlight a word, Backspace deletes the word" (WaitBuf "alpha  charlie delta ech") (Buf)
for ($k = 0; $k -lt 10 -and (Cuts) -ne 1; $k++) { [void][TP]::PostMessage($hw, $WM_PAINT, [IntPtr]0, [IntPtr]0); Start-Sleep -m 300 }
Check "the delete ran through the prompt path" ((Cuts) -eq 1) ((Get-Content $dumpFile -Encoding UTF8 | Select-String "prompt cuts").Line)

HighlightWord "charlie"; Char "X"
Check "typing over a highlight replaces it" (WaitBuf "alpha  X delta ech") (Buf)

HighlightWord "delta"; Char "a"; Char "b"; Char "c"
Check "keys typed while a highlight is being replaced all land, in order" (WaitBuf "alpha  X abc ech") (Buf)

HighlightWord "abc"
[void][TP]::PostMessage($hw, $WM_KD, [IntPtr]0x2E, [IntPtr]0); Start-Sleep -m 50   # VK_DELETE
[void][TP]::PostMessage($hw, $WM_KU, [IntPtr]0x2E, [IntPtr]0)
Check "highlight, Delete deletes it" (WaitBuf "alpha  X  ech") (Buf)

$g = FreshGeo; $f = Find $g "TRANSCRIPT-LINE-1"
if ($f) { Drag $g $f[0] $f[1] $f[0] $f[2] }
Backspace
# The caret sits where the Delete above left it (between the two spaces), so one plain
# Backspace takes one of those spaces.
Check "a highlight outside the prompt leaves the prompt alone (one Backspace)" (WaitBuf "alpha  X ech") (Buf)

$g = FreshGeo; $f = Find $g "alpha"
if ($f) { Drag $g $f[0] 0 $f[0] ($g.cols - 1) }
Backspace
Check "highlighting the whole line, mark included, and Backspace empties the prompt" (WaitBuf "") (Buf)

# Text that wraps onto several rows: highlight from a word on one row to a word on the next.
$long = (1..40 | ForEach-Object { "w{0:D2}" -f $_ }) -join " "
TypeText $long
Check "a long text wraps in the prompt" (WaitBuf $long) (Buf)
$g = FreshGeo; $f1 = Find $g "w01"
$row2 = $g.d[$f1[0] + 1].Trim()
$n = [int]($row2.Split(" ")[0].Substring(1))
$wa = "w{0:D2}" -f ($n - 3); $wb = "w{0:D2}" -f ($n + 2)
$pa = Find $g $wa; $pb = Find $g $wb
Drag $g $pa[0] $pa[1] $pb[0] $pb[2]
Backspace
$i = $long.IndexOf($wa); $j = $long.IndexOf($wb) + $wb.Length
$want = $long.Substring(0, $i) + $long.Substring($j)
Check "a highlight across a wrapped line deletes exactly what was highlighted ($wa..$wb)" (WaitBuf $want) (Buf)
} finally {
  try { $p.Refresh() } catch { }
  if (-not $p.HasExited) {
    [void][TP]::PostMessage($p.MainWindowHandle, 0x0010, [IntPtr]0, [IntPtr]0)
    if (-not $p.WaitForExit(8000)) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  }
}
$results
$fails = @($results | Where-Object { $_ -like "FAIL*" }).Count
"---"
"$($results.Count - $fails) passed, $fails failed"
