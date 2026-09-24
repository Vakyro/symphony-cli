param([string]$Exe, [string]$ArgLine, [string]$Cwd, [string]$In, [string]$Out, [string]$Err, [int]$KillAtFiles = 3, [int]$MaxSec = 900)
$p = Start-Process -FilePath $Exe -ArgumentList $ArgLine -WorkingDirectory $Cwd -RedirectStandardInput $In -RedirectStandardOutput $Out -RedirectStandardError $Err -WindowStyle Hidden -PassThru
"pid $($p.Id) start $(Get-Date -Format HH:mm:ss)"
$t0 = Get-Date
while ($true) {
  $n = @(git -C $Cwd ls-files --others --exclude-standard).Count
  if ($p.HasExited) { "TERMINO ANTES files=$n $(Get-Date -Format HH:mm:ss)"; break }
  if ($KillAtFiles -gt 0 -and $n -ge $KillAtFiles) { taskkill /F /T /PID $p.Id | Out-Null; "KILLED files=$n $(Get-Date -Format HH:mm:ss)"; break }
  if (((Get-Date) - $t0).TotalSeconds -gt $MaxSec) { taskkill /F /T /PID $p.Id | Out-Null; "TIMEOUT $(Get-Date -Format HH:mm:ss)"; break }
  Start-Sleep -Milliseconds 500
}
Start-Sleep 2
$left = @(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like "*$Cwd*" -and $_.ProcessId -ne $PID }).Count
"orphans_with_cwd_in_cmdline=$left"
git -C $Cwd status --short
