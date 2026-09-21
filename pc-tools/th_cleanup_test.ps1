# Checks the leftover-window cleanup in th_selftest.ps1 without opening a Trinidad Head window,
# so it can run anywhere PowerShell runs, Mac included. It pulls StartWin and CloseSpawned out of
# the real script, so it exercises the code the self-test actually runs, and it checks that the
# self-test still calls that cleanup from a finally block. Take the finally out and this fails.
$ErrorActionPreference = "Stop"
$src = Join-Path $PSScriptRoot "th_selftest.ps1"
# Take StartWin and CloseSpawned out of the real script by parsing it, so this test always runs
# the code the self-test runs, without executing any of its Windows-only plumbing.
$ast = [System.Management.Automation.Language.Parser]::ParseFile($src, [ref]$null, [ref]$null)
$fns = $ast.FindAll({ $args[0] -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true) |
  Where-Object { $_.Name -in @("StartWin", "CloseSpawned") }
if ($fns.Count -ne 2) { throw "expected StartWin and CloseSpawned in th_selftest.ps1, found $($fns.Count)" }
Invoke-Expression (($fns | ForEach-Object { $_.Extent.Text }) -join "`n")

$pass = 0; $fail = 0
function Check($name, $got, $want) {
  if ("$got" -eq "$want") { "PASS $name"; $script:pass++ } else { "FAIL $name (got '$got', wanted '$want')"; $script:fail++ }
}
# A window that ignores the close message, standing in for one the run left open.
function FakeWindow { Start-Process -FilePath "/bin/sleep" -ArgumentList "600" -PassThru }
if (-not (Test-Path "/bin/sleep")) {
  # On the PC: a hidden console that sits there, so nothing flashes on Matt's screen.
  function FakeWindow { Start-Process -FilePath "cmd.exe" -ArgumentList "/c pause" -WindowStyle Hidden -PassThru }
}

# 1. The run dies partway with a window still up: finally still ends it.
$log = New-Object System.Collections.Generic.List[string]
$spawned = New-Object System.Collections.Generic.List[object]
$w = FakeWindow; $spawned.Add($w)
try { throw "the run died partway" } catch { } finally { [void](CloseSpawned $spawned $log) }
$w.Refresh()
Check "an interrupted run still closes the window it left open" $w.HasExited $true
Check "and says so in the results" (($log | Where-Object { $_ -match "^NOTE closed 1 test window" }).Count) 1

# 2. Nothing left open: the cleanup stays quiet.
$log2 = New-Object System.Collections.Generic.List[string]
$done = FakeWindow; Stop-Process -Id $done.Id -Force; Start-Sleep -Seconds 1
$spawned2 = New-Object System.Collections.Generic.List[object]; $spawned2.Add($done)
$n = CloseSpawned $spawned2 $log2
Check "a clean run closes nothing" $n 0
Check "and writes no note" $log2.Count 0

# 3. StartWin records what it starts, so the cleanup can see it.
$exe = "/bin/sleep"; $py = "600"
$spawned = New-Object System.Collections.Generic.List[object]
$started = $null
try { $started = StartWin "" } catch { }
Check "StartWin adds the window it started to the list" $spawned.Count 1
if ($started) { Stop-Process -Id $started.Id -Force -ErrorAction SilentlyContinue }

# 4. The self-test still runs the cleanup on the way out, however it ends. This is the check
# that fails if the tidy-up ever drifts back to being the last line of the script.
$try = $ast.FindAll({ $args[0] -is [System.Management.Automation.Language.TryStatementAst] }, $true) |
  Where-Object { $_.Finally -and $_.Finally.Extent.Text -match "CloseSpawned" }
Check "the self-test cleans up from a finally block" ($try.Count -ge 1) $true

"--- $pass passed, $fail failed"
if ($fail -gt 0) { exit 1 }
