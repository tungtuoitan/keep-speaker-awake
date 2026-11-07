//! Silent audio stream played through the Windows `winmm` waveOut API.
//!
//! A looping buffer of zeros is handed to the driver once, so the process stays idle
//! while the speaker keeps seeing an active audio stream.

use std::mem;

use windows_sys::Win32::Media::Audio::*;
use windows_sys::Win32::Media::*;

const RATE: u32 = 22_050;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const BYTES_PER_SEC: u32 = RATE * CHANNELS as u32 * BITS as u32 / 8;

pub struct SilentStream {
    handle: HWAVEOUT,
    header: Box<WAVEHDR>,
    _buffer: Vec<u8>,
    device: u32,
    last_position: u32,
}

impl SilentStream {
    /// Start looping silence on output device `device` (index as listed by [`find_output`]).
    pub fn open(device: u32) -> Option<Self> {
        unsafe {
            let format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: CHANNELS,
                nSamplesPerSec: RATE,
                nAvgBytesPerSec: BYTES_PER_SEC,
                nBlockAlign: CHANNELS * BITS / 8,
                wBitsPerSample: BITS,
                cbSize: 0,
            };
            let mut handle: HWAVEOUT = mem::zeroed();
            if waveOutOpen(&mut handle, device, &format, 0, 0, CALLBACK_NULL) != 0 {
                return None;
            }

            let mut buffer = vec![0u8; BYTES_PER_SEC as usize]; // one second of silence
            let mut header: Box<WAVEHDR> = Box::new(mem::zeroed());
            header.lpData = buffer.as_mut_ptr();
            header.dwBufferLength = buffer.len() as u32;
            header.dwFlags = WHDR_BEGINLOOP | WHDR_ENDLOOP;
            header.dwLoops = i32::MAX as u32;

            let size = mem::size_of::<WAVEHDR>() as u32;
            waveOutPrepareHeader(handle, &mut *header, size);
            let mut stream = SilentStream { handle, header, _buffer: buffer, device, last_position: u32::MAX };
            if waveOutWrite(stream.handle, &mut *stream.header, size) != 0 {
                return None; // `Drop` releases the device
            }
            Some(stream)
        }
    }

    pub fn device(&self) -> u32 {
        self.device
    }

    /// True while the play position keeps advancing between two calls.
    pub fn is_playing(&mut self) -> bool {
        unsafe {
            let mut time: MMTIME = mem::zeroed();
            time.wType = TIME_BYTES;
            if waveOutGetPosition(self.handle, &mut time, mem::size_of::<MMTIME>() as u32) != 0 {
                return false;
            }
            let position = time.u.cb;
            let advanced = position != self.last_position;
            self.last_position = position;
            advanced
        }
    }
}

impl Drop for SilentStream {
    fn drop(&mut self) {
        unsafe {
            waveOutReset(self.handle);
            waveOutUnprepareHeader(self.handle, &mut *self.header, mem::size_of::<WAVEHDR>() as u32);
            waveOutClose(self.handle);
        }
    }
}

/// Find an output device whose name contains `name` (case-insensitive).
/// Prefers the stereo endpoint over the Hands-Free one. Returns `(index, full name)`.
pub fn find_output(name: &str) -> Option<(u32, String)> {
    let needle = name.to_lowercase();
    let mut found = Vec::new();
    unsafe {
        for index in 0..waveOutGetNumDevs() {
            let mut caps: WAVEOUTCAPSW = mem::zeroed();
            if waveOutGetDevCapsW(index as usize, &mut caps, mem::size_of::<WAVEOUTCAPSW>() as u32) != 0 {
                continue;
            }
            let raw = caps.szPname; // packed struct: copy before borrowing
            let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            let device_name = String::from_utf16_lossy(&raw[..len]);
            if device_name.to_lowercase().contains(&needle) {
                found.push((index, device_name));
            }
        }
    }
    found.sort_by_key(|(_, n)| n.to_lowercase().contains("hands-free"));
    found.into_iter().next()
}
