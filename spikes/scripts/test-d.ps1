# Test D (P01.S6): A trabaja, se mata sin cleanup, B continúa solo con el checkpoint.
# uso: test-d.ps1 <t1|t2|t3> <claude|codex>   (el primero es A; B es el otro)
param([string]$Task, [string]$First)
$ErrorActionPreference = 'Continue'
$S = "$env:USERPROFILE\symphony-spike"
$hook = "$S\spike-hook.exe"
$events = "$S\d-events.jsonl"
$codex = "$env:APPDATA\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe"
$Second = if ($First -eq 'claude') { 'codex' } else { 'claude' }
$tag = "$Task-$First"
$wt = "$S\d-$tag"
$log = "$S\d-$tag.log"
function Log($m) { $line = "[{0:HH:mm:ss}] $m" -f (Get-Date); $line; Add-Content $log $line }

# Worktree nuevo desde la base
git -C "$S\d-repo" worktree remove --force $wt 2>$null
git -C "$S\d-repo" branch -D "d/$tag" 2>$null
git -C "$S\d-repo" worktree add -q $wt -b "d/$tag" main
"" | Set-Content $log

# Hooks: Claude via --settings, Codex via -c (sintaxis PowerShell, LEARNINGS Test B)
$hk = ($S -replace '\\','/') + '/spike-hook.exe'
$h = @{ type='command'; command="`"$hk`" hook claude"; timeout=30 }
$ev = [ordered]@{}
foreach ($e in 'SessionStart','UserPromptSubmit','PreToolUse','PostToolUse','PostToolUseFailure','Stop','StopFailure') { $ev[$e] = @(@{ hooks=@($h) }) }
@{ hooks=$ev } | ConvertTo-Json -Depth 8 -Compress | Set-Content "$S\d-claude-settings.json"
$codexHooks = foreach ($e in 'SessionStart','UserPromptSubmit','PreToolUse','PostToolUse','Stop') { '-c'; "hooks.$e=[{hooks=[{type=`"command`",command=`"& '$hk' hook codex`",timeout=30}]}]" }

function Start-Agent($provider, $agentId, $promptFile, $outFile) {
    $env:SYMPHONY_AGENT_ID = $agentId
    if ($provider -eq 'claude') {
        $exe = 'cmd.exe'
        $args = "/c claude.cmd -p --settings `"$S\d-claude-settings.json`" --output-format stream-json --verbose --model sonnet --permission-mode acceptEdits --allowedTools Bash Read Write Edit"
    } else {
        $exe = $codex
        $args = (@('exec', '--json', '-s', 'workspace-write', '--dangerously-bypass-hook-trust') + ($codexHooks | ForEach-Object { if ($_ -eq '-c') { '-c' } else { '"' + ($_ -replace '"','\"') + '"' } }) + @('-')) -join ' '
    }
    Start-Process -FilePath $exe -ArgumentList $args -WorkingDirectory $wt -RedirectStandardInput $promptFile -RedirectStandardOutput $outFile -RedirectStandardError "$outFile.err" -WindowStyle Hidden -PassThru
}

function AgentEvents($agentId) {
    if (-not (Test-Path $events)) { return @() }
    Get-Content $events | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.agent_id -eq $agentId }
}

function Snapshot() {
    $h = @{}
    Push-Location $wt
    $files = @(git ls-files --others --exclude-standard) + @(git diff --name-only)
    foreach ($f in $files | Where-Object { $_ }) { $h[$f] = (Get-FileHash $f -Algorithm SHA256).Hash }
    Pop-Location
    $h
}

# --- A
$aId = "d-$tag-A"
Copy-Item "$S\goals\$Task.md" "$S\d-$tag.goal.txt"
$pA = Start-Agent $First $aId "$S\d-$tag.goal.txt" "$S\d-$tag-A.jsonl"
Log "A=$First pid=$($pA.Id) arrancó"
$sw = [Diagnostics.Stopwatch]::StartNew()
$killed = $false; $firstEdit = $null
while (-not $pA.HasExited -and $sw.Elapsed.TotalMinutes -lt 12) {
    Start-Sleep -Milliseconds 700
    $evs = @(AgentEvents $aId)
    $edits = @($evs | Where-Object { $_.event -eq 'FileModified' })
    if ($edits.Count -ge 1 -and -not $firstEdit) { $firstEdit = $sw.Elapsed.TotalSeconds }
    $kill = switch ($Task) {
        't1' { $edits.Count -ge 2 }
        't2' { $edits.Count -ge 2 -and @($evs | Where-Object { $_.event -eq 'CommandRequested' -and $_.command -match 'test' }).Count -ge 1 }
        't3' { $firstEdit -and ($sw.Elapsed.TotalSeconds - $firstEdit) -ge 45 }
    }
    if ($kill) {
        if ($Task -eq 't2') { Start-Sleep -Seconds 3 }  # que los tests ya estén corriendo
        taskkill /F /T /PID $pA.Id | Out-Null
        $killed = $true
        break
    }
}
Log ("A {0} a los {1:N0} s · eventos={2} · ediciones={3}" -f ($(if ($killed) { 'MUERTO' } else { 'terminó/timeout sin kill' })), $sw.Elapsed.TotalSeconds, @(AgentEvents $aId).Count, @(AgentEvents $aId | Where-Object { $_.event -eq 'FileModified' }).Count)
Start-Sleep -Seconds 3
$orphans = @(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match [regex]::Escape($wt) -and $_.Name -match 'node|claude|codex' })
Log "huérfanos tras kill: $($orphans.Count)"
$before = Snapshot
Log ("A dejó: " + (($before.Keys | Sort-Object) -join ', '))

# --- Handoff solo desde el checkpoint
& $hook handoff "$S\checkpoints\$aId.json" "$S\goals\$Task.md" | Set-Content "$S\d-$tag.handoff.md" -Encoding utf8
Log ("handoff: {0:N0} bytes" -f (Get-Item "$S\d-$tag.handoff.md").Length)

# --- B
$bId = "d-$tag-B"
$swB = [Diagnostics.Stopwatch]::StartNew()
$pB = Start-Agent $Second $bId "$S\d-$tag.handoff.md" "$S\d-$tag-B.jsonl"
Log "B=$Second pid=$($pB.Id) arrancó"
$null = $pB.WaitForExit(20 * 60 * 1000)
if (-not $pB.HasExited) { taskkill /F /T /PID $pB.Id | Out-Null; Log "B TIMEOUT" }
Log ("B terminó en {0:N0} s · eventos={1} · comandos={2}" -f $swB.Elapsed.TotalSeconds, @(AgentEvents $bId).Count, @(AgentEvents $bId | Where-Object { $_.event -eq 'CommandFinished' }).Count)

# --- Resultado
Push-Location $wt
$test = (npm test 2>&1) -join "`n"
Pop-Location
$pass = if ($test -match '# pass (\d+)') { $Matches[1] } else { '?' }
$fail = if ($test -match '# fail (\d+)') { $Matches[1] } else { '?' }
Log "npm test final: pass=$pass fail=$fail"
$after = Snapshot
$same = @($before.Keys | Where-Object { $after[$_] -eq $before[$_] })
$changed = @($before.Keys | Where-Object { $after.ContainsKey($_) -and $after[$_] -ne $before[$_] })
$gone = @($before.Keys | Where-Object { -not $after.ContainsKey($_) })
Log ("archivos de A: {0} idénticos [{1}] · {2} modificados [{3}] · {4} borrados [{5}]" -f $same.Count, ($same -join ','), $changed.Count, ($changed -join ','), $gone.Count, ($gone -join ','))
$tests = @(Get-ChildItem "$wt\test" -Filter *.test.js | ForEach-Object Name) -join ','
Log "tests al final: $tests"
