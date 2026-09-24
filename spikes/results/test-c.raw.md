# Test C — salida cruda

```text

## claude-hold120-to180  (exit=0, total 142 s)
1790271800217  CommandRequested PreToolUse         npm test .
1790271800219  HoldStarted      PreToolUse         npm test .
1790271920220  HoldReleased     PreToolUse         npm test .
1790271929409  CommandFinished  PostToolUse        npm test .
1790271931372  TurnFinished     Stop               
--- respuesta final:
The test ran successfully in under a second and output "tests ok".

## codex-hold120-to180  (exit=0, total 282 s)
1790271953706  CommandRequested PreToolUse         npm test .
1790271953707  HoldStarted      PreToolUse         npm test .
1790272073708  HoldReleased     PreToolUse         npm test .
1790272082626  CommandRequested PreToolUse         npm test .
1790272082627  HoldStarted      PreToolUse         npm test .
1790272202629  HoldReleased     PreToolUse         npm test .
1790272210155  CommandFinished  PostToolUse        npm test .
1790272213791  TurnFinished     Stop               
--- respuesta final:
`npm test .` ran successfully in about 7 seconds and output `tests ok`.

## claude-hold60-to30  (exit=0, total 49 s)
1790272223175  CommandRequested PreToolUse         npm test .
1790272223176  HoldStarted      PreToolUse         npm test .
1790272259327  CommandFinished  PostToolUse        npm test .
1790272262233  TurnFinished     Stop               
--- respuesta final:
The test ran successfully in under a second, printing "tests ok"—a simple Node.js script that echoes a success message.

## codex-hold60-to30  (exit=0, total 78 s)
1790272281815  CommandRequested PreToolUse         npm test .
1790272281816  HoldStarted      PreToolUse         npm test .
1790272320393  CommandFinished  PostToolUse        npm test .
1790272324556  TurnFinished     Stop               
1790272341818  HoldReleased     PreToolUse         npm test .
--- respuesta final:
`npm test .` ran successfully in about 6 seconds, outputting `tests ok`.

## codex-hold30-to180  (exit=0, total 61 s)
1790272612783  CommandRequested PreToolUse         npm test .
1790272612784  HoldStarted      PreToolUse         npm test .
1790272642785  HoldReleased     PreToolUse         npm test .
1790272652423  CommandFinished  PostToolUse        npm test .
1790272655148  TurnFinished     Stop               
--- respuesta final:
`npm test .` ran successfully in about 7 seconds, outputting `tests ok`.

## codex-hold60-to180  (exit=0, total 93 s)
1790272675009  CommandRequested PreToolUse         npm test .
1790272675010  HoldStarted      PreToolUse         npm test .
1790272735012  HoldReleased     PreToolUse         npm test .
1790272744910  CommandFinished  PostToolUse        npm test .
1790272748315  TurnFinished     Stop               
--- respuesta final:
`npm test .` ran successfully in about 6.9 seconds; output: `tests ok`.
```
