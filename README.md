# keep-speaker-awake

Many Bluetooth speakers power off after a few minutes of silence, and some don't reconnect
when you switch them back on. This tiny Windows tool fixes both:

- **Stays awake** – plays an inaudible stream of silence to the speaker, so it never idles out.
- **Reconnects** – when the speaker is paired but not connected (or "connected" with no audio
  device), it re-enables the Audio Sink service, the same as clicking *Connect*.

~2 MB of RAM, ~0% CPU, no dependencies, no admin rights.

## Use

1. Pair your speaker with Windows as usual.
2. Download `keep-speaker-awake.exe` from [Releases](../../releases) (or `cargo build --release`)
   and put it somewhere permanent.
3. Run it once, with part of your speaker's name:

```powershell
.\keep-speaker-awake.exe install EMBERTON
```

That registers a scheduled task that starts at every logon and starts it now.
To remove it: `.\keep-speaker-awake.exe uninstall`.

Try it without installing: `.\keep-speaker-awake.exe EMBERTON` (add `--log file.txt` to see what it does).

## How it works

Every 15 s it looks up the paired device by name, checks whether it is connected, and
keeps a looping silent buffer playing on the matching audio output (`winmm` waveOut — the
driver loops the buffer, so the process stays idle). It only acts after a state has held for
two checks in a row, so it never interferes while Windows is reconnecting by itself.

## Limits

- Windows 10/11 only. The speaker must already be paired.
- The speaker stays on until you power it off or its battery runs out – plug it in if it
  stays on all day.
- Tested with a Marshall EMBERTON; other A2DP speakers should work, no guarantees.

## License

MIT
