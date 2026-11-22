//! Phone via PC: the phone's Bluetooth audio link.
//!
//! The phone plays into this PC as an A2DP source. Connecting the device
//! alone brings no audio. The A2DP profile must connect too.

use std::process::Command;
use std::thread;

use crate::bluez::{known_devices, profile_call};

/// Audio Source (A2DP source) service UUID.
pub const A2DP_SOURCE: &str = "0000110a-0000-1000-8000-00805f9b34fb";
/// AVRCP controller target UUID. Keeps the link up when A2DP drops.
pub const AVRCP_TARGET: &str = "0000110c-0000-1000-8000-00805f9b34fb";

/// The phone address: the setting if set, else the first paired phone that
/// offers an A2DP source.
pub fn resolve(setting: &str) -> Option<String> {
    if !setting.is_empty() {
        return Some(setting.to_ascii_uppercase());
    }
    known_devices()
        .ok()?
        .into_iter()
        .find(|d| d.icon == "phone" && d.uuids.iter().any(|u| u == A2DP_SOURCE))
        .map(|d| d.address)
}

/// Connect the A2DP source profile. AVRCP follows in the background.
pub fn link(address: &str) -> Result<(), String> {
    profile_call(address, "ConnectProfile", A2DP_SOURCE)?;
    let address = address.to_owned();
    thread::spawn(move || {
        let _ = profile_call(&address, "ConnectProfile", AVRCP_TARGET);
    });
    Ok(())
}

/// Drop the A2DP source profile. The phone falls back to its own speaker.
/// AVRCP stays up, so the next link is fast.
pub fn unlink(address: &str) -> Result<(), String> {
    profile_call(address, "DisconnectProfile", A2DP_SOURCE)
}

/// Is the phone's audio card present in PipeWire or PulseAudio?
pub fn audio_linked(address: &str) -> bool {
    let card = format!("bluez_card.{}", address.to_ascii_uppercase().replace(':', "_"));
    Command::new("pactl")
        .args(["list", "cards", "short"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(&card))
        .unwrap_or(false)
}
