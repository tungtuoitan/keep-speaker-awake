//! keep-speaker-awake — stop a Bluetooth speaker from powering off when idle, and
//! reconnect it automatically when it is switched on. See README.md.
#![windows_subsystem = "windows"] // no console window when run in the background

mod audio;
mod bluetooth;
mod install;

use std::io::Write;
use std::{thread, time::Duration, time::Instant};

use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

/// Seconds between checks.
const CHECK_EVERY: Duration = Duration::from_secs(15);
/// Minimum pause between two reconnect attempts.
const RECONNECT_EVERY: Duration = Duration::from_secs(30);
/// Consecutive checks a state must hold before we act, so we never interfere while
/// Windows is reconnecting by itself or the audio endpoint is still coming up.
const STABLE_CHECKS: u32 = 2;

const USAGE: &str = "\
keep-speaker-awake - keep a Bluetooth speaker awake and auto-reconnect it

USAGE
  keep-speaker-awake <name> [--log FILE]    run in the foreground
  keep-speaker-awake install <name>         run at every logon (scheduled task) and start now
  keep-speaker-awake uninstall              remove the scheduled task

<name> is part of the speaker's Bluetooth name, e.g. EMBERTON. The speaker must be
paired with this PC first.
";

/// Print to the parent terminal (this is a GUI-subsystem exe) or, if there is none, a message box.
fn show(text: &str) {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            if let Ok(mut out) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                let _ = writeln!(out, "{text}");
                return;
            }
        }
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide("keep-speaker-awake").as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] | ["-h" | "--help"] => show(USAGE),
        ["install", name] => report(install::install(name), &format!("Installed. Keeping \"{name}\" awake at every logon.")),
        ["uninstall"] => report(install::uninstall(), "Uninstalled."),
        [name, rest @ ..] => {
            let log = match rest {
                ["--log", file] => std::fs::OpenOptions::new().create(true).append(true).open(file).ok(),
                _ => None,
            };
            run(name, log);
        }
    }
}

fn report(result: Result<(), String>, ok: &str) {
    match result {
        Ok(()) => show(ok),
        Err(e) => show(&format!("Error: {e}")),
    }
}

fn run(name: &str, mut log: Option<std::fs::File>) -> ! {
    let mut note = |line: &str| {
        if let Some(f) = &mut log {
            let _ = writeln!(f, "{line}");
        }
    };

    let mut address: Option<u64> = None;
    let mut stream: Option<audio::SilentStream> = None;
    let mut last_reconnect: Option<Instant> = None;
    // How many checks in a row each condition has held.
    let (mut disconnected, mut no_endpoint, mut endpoint_ready) = (0u32, 0u32, 0u32);

    loop {
        address = address.or_else(|| bluetooth::find_paired(name));
        let state = address.and_then(bluetooth::state);
        let paired = state.is_some_and(|s| s.paired);
        let connected = state.is_some_and(|s| s.connected);
        let output = audio::find_output(name);

        disconnected = if connected { 0 } else { disconnected + 1 };
        no_endpoint = if connected && output.is_none() { no_endpoint + 1 } else { 0 };
        endpoint_ready = if connected && output.is_some() { endpoint_ready + 1 } else { 0 };

        // Reconnect when paired but not connected, or "connected" without an audio endpoint.
        let stuck = (paired && !connected && disconnected >= STABLE_CHECKS) || no_endpoint >= STABLE_CHECKS;
        if let (true, Some(addr)) = (stuck, address) {
            if last_reconnect.is_none_or(|t| t.elapsed() >= RECONNECT_EVERY) {
                note(&format!("reconnect -> code {}", bluetooth::reconnect(addr)));
                last_reconnect = Some(Instant::now());
                (disconnected, no_endpoint) = (0, 0);
            }
        }

        // Keep the silent stream on the speaker's output while it is connected.
        if let Some(s) = &mut stream {
            if !connected || output.as_ref().is_none_or(|o| o.0 != s.device()) || !s.is_playing() {
                note("stream closed");
                stream = None;
            }
        }
        if let (None, Some((index, device_name)), true) = (&stream, &output, endpoint_ready >= STABLE_CHECKS) {
            stream = audio::SilentStream::open(*index);
            note(&format!("stream on {device_name}: {}", if stream.is_some() { "ok" } else { "failed" }));
        }

        thread::sleep(CHECK_EVERY);
    }
}
