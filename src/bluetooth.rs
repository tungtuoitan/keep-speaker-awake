//! Bluetooth helpers: find a paired speaker by name, read its state, force a reconnect.

use std::{mem, ptr};

use windows_sys::core::GUID;
use windows_sys::Win32::Devices::Bluetooth::*;

/// Audio Sink (A2DP) service class: 0000110B-0000-1000-8000-00805F9B34FB
const A2DP_SINK: GUID = GUID::from_u128(0x0000110B_0000_1000_8000_00805F9B34FB);

#[derive(Clone, Copy)]
pub struct State {
    pub paired: bool,
    pub connected: bool,
}

fn device_info(address: u64) -> BLUETOOTH_DEVICE_INFO {
    let mut info: BLUETOOTH_DEVICE_INFO = unsafe { mem::zeroed() };
    info.dwSize = mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32;
    info.Address.Anonymous.ullLong = address;
    info
}

/// Address of the first paired device whose name contains `name` (case-insensitive).
pub fn find_paired(name: &str) -> Option<u64> {
    let needle = name.to_lowercase();
    unsafe {
        let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
            dwSize: mem::size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
            fReturnAuthenticated: 1,
            fReturnRemembered: 1,
            fReturnUnknown: 0,
            fReturnConnected: 1,
            fIssueInquiry: 0, // list known devices only, no radio scan
            cTimeoutMultiplier: 0,
            hRadio: ptr::null_mut(),
        };
        let mut info: BLUETOOTH_DEVICE_INFO = mem::zeroed();
        info.dwSize = mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32;

        let find = BluetoothFindFirstDevice(&params, &mut info);
        if find.is_null() {
            return None;
        }
        let mut result = None;
        loop {
            let raw = info.szName; // packed struct: copy before borrowing
            let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            if String::from_utf16_lossy(&raw[..len]).to_lowercase().contains(&needle) {
                result = Some(info.Address.Anonymous.ullLong);
                break;
            }
            if BluetoothFindNextDevice(find, &mut info) == 0 {
                break;
            }
        }
        BluetoothFindDeviceClose(find);
        result
    }
}

pub fn state(address: u64) -> Option<State> {
    unsafe {
        let mut info = device_info(address);
        if BluetoothGetDeviceInfo(ptr::null_mut(), &mut info) != 0 {
            return None;
        }
        Some(State { paired: info.fRemembered != 0, connected: info.fConnected != 0 })
    }
}

/// Toggle the speaker's Audio Sink service off and on; Windows then reconnects it
/// (same effect as pressing "Connect"). Returns the Win32 error code (0 = success).
pub fn reconnect(address: u64) -> u32 {
    unsafe {
        let info = device_info(address);
        BluetoothSetServiceState(ptr::null_mut(), &info, &A2DP_SINK, BLUETOOTH_SERVICE_DISABLE);
        BluetoothSetServiceState(ptr::null_mut(), &info, &A2DP_SINK, BLUETOOTH_SERVICE_ENABLE)
    }
}
