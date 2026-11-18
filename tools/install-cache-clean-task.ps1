# Registers (or replaces) the daily `pdfcer-clean-target-cache` task, which runs
# tools/clean-target-cache.py at 04:00 so build outputs are pruned whether or not
# a build or gate run happened that day. Current user, no elevation.
# Remove with: Unregister-ScheduledTask -TaskName pdfcer-clean-target-cache
$repo = Split-Path -Parent $PSScriptRoot
$python = (Get-Command python -ErrorAction Stop).Source
$action = New-ScheduledTaskAction -Execute $python `
    -Argument "`"$repo\tools\clean-target-cache.py`"" -WorkingDirectory $repo
$trigger = New-ScheduledTaskTrigger -Daily -At 4am
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -DontStopIfGoingOnBatteries `
    -AllowStartIfOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 1)
Register-ScheduledTask -TaskName pdfcer-clean-target-cache -Action $action -Trigger $trigger `
    -Settings $settings -Description "Prune superseded pdfcer build outputs" -Force | Out-Null
Get-ScheduledTask -TaskName pdfcer-clean-target-cache | Select-Object TaskName, State
