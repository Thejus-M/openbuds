//! Phone sound through the PC is quiet at the buds' normal volume. While the
//! phone is linked, raise the buds sink to a boost level, then put it back.
//!
//! The saved level lives in a file, so a daemon restart mid-boost still
//! restores it later. If the user moved the volume during the boost, their
//! new level is kept.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::ipc::state_dir;

fn file() -> PathBuf {
    state_dir().join("sink-boost")
}

fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// First `NN%` in the text, as a number.
fn percent(text: &str) -> Option<u32> {
    text.split_whitespace()
        .find(|t| t.ends_with('%'))
        .and_then(|t| t.trim_end_matches('%').parse().ok())
}

/// The default sink, only when it is a Bluetooth output.
fn buds_sink() -> Option<String> {
    let sink = pactl(&["get-default-sink"])?.trim().to_owned();
    sink.starts_with("bluez_output.").then_some(sink)
}

/// Raise the buds sink to `target` percent, unless a boost is already saved
/// or the sink is already louder.
pub fn boost(target: u32) {
    if fs::read_to_string(file()).map(|s| !s.trim().is_empty()).unwrap_or(false) {
        return;
    }
    let Some(sink) = buds_sink() else { return };
    let Some(volume) = pactl(&["get-sink-volume", &sink]).and_then(|t| percent(&t)) else {
        return;
    };
    if volume >= target {
        return;
    }
    let _ = fs::create_dir_all(state_dir());
    if fs::write(file(), format!("{sink} {volume} {target}\n")).is_ok() {
        let _ = pactl(&["set-sink-volume", &sink, &format!("{target}%")]);
    }
}

/// Put the sink back, unless the user changed the volume during the boost.
pub fn restore() {
    let Ok(saved) = fs::read_to_string(file()) else { return };
    let mut parts = saved.split_whitespace();
    let (Some(sink), Some(level), Some(target)) = (parts.next(), parts.next(), parts.next()) else {
        let _ = fs::remove_file(file());
        return;
    };
    let Some(sinks) = pactl(&["list", "short", "sinks"]) else { return };
    if !sinks.contains(sink) {
        // The buds sink is gone: keep the file and restore on a later call.
        return;
    }
    let current = pactl(&["get-sink-volume", sink]).and_then(|t| percent(&t));
    if current == target.parse::<u32>().ok() {
        let _ = pactl(&["set-sink-volume", sink, &format!("{level}%")]);
    }
    let _ = fs::remove_file(file());
}
