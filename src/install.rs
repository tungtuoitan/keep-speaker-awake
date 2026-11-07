//! Register / remove the "run at logon" scheduled task.

use std::process::Command;

const TASK_NAME: &str = "KeepSpeakerAwake";

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn powershell(script: &str) -> Result<(), String> {
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .status()
        .map_err(|e| format!("cannot run powershell: {e}"))?;
    status.success().then_some(()).ok_or_else(|| "PowerShell command failed".into())
}

/// Run `<this exe> "<name>"` at every logon, also on battery, with no time limit.
pub fn install(name: &str) -> Result<(), String> {
    if name.contains('"') {
        return Err("speaker name must not contain a double quote".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         $action=New-ScheduledTaskAction -Execute {exe} -Argument {arg}; \
         $trigger=New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME; \
         $settings=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries \
           -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew \
           -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1); \
         Stop-ScheduledTask -TaskName {task} -ErrorAction SilentlyContinue; \
         Register-ScheduledTask -TaskName {task} -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null; \
         Start-ScheduledTask -TaskName {task}",
        exe = ps_quote(&exe.to_string_lossy()),
        arg = ps_quote(&format!("\"{name}\"")),
        task = ps_quote(TASK_NAME),
    );
    powershell(&script)
}

pub fn uninstall() -> Result<(), String> {
    let task = ps_quote(TASK_NAME);
    powershell(&format!(
        "Stop-ScheduledTask -TaskName {task} -ErrorAction SilentlyContinue; \
         Unregister-ScheduledTask -TaskName {task} -Confirm:$false"
    ))
}
