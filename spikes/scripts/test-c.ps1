# Test C (P01.S5): retener `npm test` en PreToolUse y ver qué hace cada CLI.
$ErrorActionPreference = 'Continue'
$S = "$env:USERPROFILE\symphony-spike"
$wt = "$S\wt3"
$codex = "$env:APPDATA\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe"
$prompt = 'Run the command: npm test . Then reply with one sentence saying whether it ran, how long it seemed to take, and its output.'
$env:SPIKE_HOLD_MATCH = 'npm test'
Set-Location $wt

function Run($provider, $hold, $timeout) {
    $tag = "$provider-hold$hold-to$timeout"
    $env:SYMPHONY_AGENT_ID = "c-$tag"
    $env:SPIKE_HOLD_SECS = "$hold"
    $events = Get-Content "$S\events.jsonl"
    $n0 = $events.Count
    $sw = [Diagnostics.Stopwatch]::StartNew()
    if ($provider -eq 'claude') {
        $hk = ($S -replace '\\','/') + '/spike-hook.exe'
        $h = @{ type='command'; command="`"$hk`" hook claude"; timeout=$timeout }
        $ev = [ordered]@{}
        foreach ($e in 'PreToolUse','PostToolUse','PostToolUseFailure','Stop') { $ev[$e] = @(@{ hooks=@($h) }) }
        @{ hooks=$ev } | ConvertTo-Json -Depth 8 -Compress | Set-Content "$S\c-settings.json"
        $prompt | & claude.cmd -p --settings "$S\c-settings.json" --output-format stream-json --verbose --model haiku --allowedTools Bash > "$S\c-$tag.jsonl" 2>&1
    } else {
        $hk = "& 'C:/Users/Latitude 7390/symphony-spike/spike-hook.exe' hook codex"
        $cs = foreach ($e in 'PreToolUse','PostToolUse','Stop') { '-c'; "hooks.$e=[{hooks=[{type=`"command`",command=`"$hk`",timeout=$timeout}]}]" }
        $prompt | & $codex exec --json -m gpt-5.6-luna -s workspace-write --dangerously-bypass-hook-trust @cs - > "$S\c-$tag.jsonl" 2>&1
    }
    $sw.Stop()
    "`n## $tag  (exit=$LASTEXITCODE, total {0:N0} s)" -f $sw.Elapsed.TotalSeconds
    Get-Content "$S\events.jsonl" | Select-Object -Skip $n0 | ForEach-Object {
        $o = $_ | ConvertFrom-Json
        "{0}  {1,-16} {2,-18} {3}" -f $o.ts_ms, $o.event, $o.cli_event, $o.command
    }
    "--- respuesta final:"
    if ($provider -eq 'claude') {
        Select-String -Path "$S\c-$tag.jsonl" -Pattern '"type":"result"' | ForEach-Object { ($_.Line | ConvertFrom-Json).result }
    } else {
        Get-Content "$S\c-$tag.jsonl" | Where-Object { $_ -match '"agent_message"' } | Select-Object -Last 1 | ForEach-Object { ($_ | ConvertFrom-Json).item.text }
    }
}

Run claude 120 180
Run codex 120 180
Run claude 60 30
Run codex 60 30
