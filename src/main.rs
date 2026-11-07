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
/// How long each silent pulse lasts in `--pulse` mode.
const PULSE_LENGTH: Duration = Duration::from_secs(4);

const USAGE: &str = "\
keep-speaker-awake - keep a Bluetooth speaker awake and auto-reconnect it

USAGE
  keep-speaker-awake <name> [--pulse MIN] [--log FILE]   run in the foreground
  keep-speaker-awake install <name> [--pulse MIN]        run at every logon (scheduled task) and start now
  keep-speaker-awake uninstall                           remove the scheduled task

<name>        part of the speaker's Bluetooth name, e.g. EMBERTON (must be paired first).
--pulse MIN   play silence for a few seconds every MIN minutes instead of continuously
              (saves speaker battery; MIN must be shorter than the speaker's auto-off time).
--log FILE    write what the program does to FILE.
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

struct Options {
    name: String,
    pulse_minutes: Option<u64>,
    log: Option<String>,
}

/// `<name> [--pulse MINUTES] [--log FILE]`
fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options { name: String::new(), pulse_minutes: None, log: None };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--pulse" => {
                let minutes = it.next().and_then(|v| v.parse::<u64>().ok()).filter(|&m| m > 0);
                options.pulse_minutes = Some(minutes.ok_or("--pulse needs a number of minutes (> 0)")?);
            }
            "--log" => options.log = Some(it.next().ok_or("--log needs a file")?.clone()),
            name if options.name.is_empty() && !name.starts_with("--") => options.name = name.to_string(),
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    if options.name.is_empty() {
        return Err("missing speaker name".into());
    }
    Ok(options)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help") => show(USAGE),
        Some("uninstall") => report(install::uninstall(), "Uninstalled."),
        Some("install") => match parse(&args[1..]) {
            Ok(o) => report(
                install::install(&o.name, o.pulse_minutes),
                &format!("Installed. Keeping \"{}\" awake at every logon.", o.name),
            ),
            Err(e) => show(&format!("Error: {e}\n\n{USAGE}")),
        },
        Some(_) => match parse(&args) {
            Ok(o) => run(o),
            Err(e) => show(&format!("Error: {e}\n\n{USAGE}")),
        },
    }
}

fn report(result: Result<(), String>, ok: &str) {
    match result {
        Ok(()) => show(ok),
        Err(e) => show(&format!("Error: {e}")),
    }
}

fn run(options: Options) -> ! {
    let name = options.name.as_str();
    let pulse = options.pulse_minutes.map(|m| Duration::from_secs(m * 60));
    let mut log = options
        .log
        .and_then(|f| std::fs::OpenOptions::new().create(true).append(true).open(f).ok());
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
    // Pulse mode: the stream is open only for PULSE_LENGTH out of every `pulse`.
    let mut next_pulse = Instant::now();
    let mut pulse_end = Instant::now();

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

        // Keep the silent stream on the speaker's output while it is connected
        // (continuously, or in short pulses when --pulse is set).
        let now = Instant::now();
        let pulse_over = pulse.is_some() && now >= pulse_end;
        if let Some(s) = &mut stream {
            if !connected || pulse_over || output.as_ref().is_none_or(|o| o.0 != s.device()) || !s.is_playing() {
                note("stream closed");
                stream = None;
            }
        }
        let pulse_due = pulse.is_none() || now >= next_pulse;
        if let (None, Some((index, device_name)), true, true) =
            (&stream, &output, endpoint_ready >= STABLE_CHECKS, pulse_due)
        {
            stream = audio::SilentStream::open(*index);
            note(&format!("stream on {device_name}: {}", if stream.is_some() { "ok" } else { "failed" }));
            if let Some(every) = pulse {
                next_pulse = now + every;
                pulse_end = now + PULSE_LENGTH;
            }
        }

        // Sleep until the next check, or until the pulse starts/ends if that comes sooner.
        let mut wait = CHECK_EVERY;
        if pulse.is_some() {
            let deadline = if stream.is_some() { pulse_end } else { next_pulse };
            wait = wait.min(deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(200)));
        }
        thread::sleep(wait);
    }
}
