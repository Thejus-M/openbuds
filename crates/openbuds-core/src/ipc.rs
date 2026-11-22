//! Shared files and the socket between `openbudsd`, `openbuds-tray` and the CLI.
//!
//! State: `$XDG_RUNTIME_DIR/openbuds/state.json`, rewritten by the daemon.
//! Control: `$XDG_RUNTIME_DIR/openbuds/ipc.sock`. One line per request:
//! `via-pc on|off`, `use-here`, `send-to-phone`, `connect`, `disconnect`.
//! The reply is one line: `ok` or `error: <message>`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Runtime directory. Falls back to `/tmp/openbuds-<uid>` without `XDG_RUNTIME_DIR`.
pub fn runtime_dir() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("openbuds"),
        _ => PathBuf::from(format!("/tmp/openbuds-{}", unsafe { libc::getuid() })),
    }
}

pub fn state_path() -> PathBuf {
    runtime_dir().join("state.json")
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("ipc.sock")
}

/// Persistent state: `$XDG_STATE_HOME/openbuds`, else `~/.local/state/openbuds`.
pub fn state_dir() -> PathBuf {
    match std::env::var("XDG_STATE_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("openbuds"),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".local/state/openbuds"),
    }
}

/// Everything the tray and the CLI show. Written by the daemon.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StateDoc {
    /// Unix seconds of the last write.
    pub updated_at: u64,
    pub connected: bool,
    pub name: Option<String>,
    pub address: Option<String>,
    /// Source: `none`, `bluez` or `samsung-spp`.
    pub source: String,
    pub battery: Option<u8>,
    pub battery_left: Option<u8>,
    pub battery_right: Option<u8>,
    pub battery_case: Option<u8>,
    pub charging: bool,
    pub worn_left: bool,
    pub worn_right: bool,
    pub noise_mode: Option<String>,
    pub phone_name: Option<String>,
    pub phone_battery: Option<u8>,
    pub phone_charging: bool,
    /// Phone via PC is on.
    pub via_pc: bool,
    /// `pc`, `call`, `sent`, `taken` or `none`.
    pub owner: String,
    /// `pc`, `phone` or `none`.
    pub in_flight: String,
    /// Phone's audio card is in PipeWire/PulseAudio.
    pub phone_audio_linked: bool,
    /// Last error from a connect or a control request, empty when none.
    pub last_error: String,
}

/// Write the state atomically: temp file, then rename.
pub fn write_state(doc: &StateDoc) -> std::io::Result<()> {
    let path = state_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(doc).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

/// Read the state the daemon last wrote. `None` when the daemon is not running.
pub fn read_state() -> Option<StateDoc> {
    let text = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// A control request from a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ViaPc(bool),
    UseHere,
    SendToPhone,
    Connect,
    Disconnect,
}

impl Command {
    pub fn parse(line: &str) -> Option<Command> {
        let mut words = line.split_whitespace();
        Some(match (words.next()?, words.next()) {
            ("via-pc", Some("on")) => Command::ViaPc(true),
            ("via-pc", Some("off")) => Command::ViaPc(false),
            ("use-here", None) => Command::UseHere,
            ("send-to-phone", None) => Command::SendToPhone,
            ("connect", None) => Command::Connect,
            ("disconnect", None) => Command::Disconnect,
            _ => return None,
        })
    }

    pub fn line(self) -> &'static str {
        match self {
            Command::ViaPc(true) => "via-pc on",
            Command::ViaPc(false) => "via-pc off",
            Command::UseHere => "use-here",
            Command::SendToPhone => "send-to-phone",
            Command::Connect => "connect",
            Command::Disconnect => "disconnect",
        }
    }
}

/// Send one command to the daemon and return its reply line.
pub fn send(command: Command) -> Result<String, String> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path())
        .map_err(|_| "openbudsd is not running".to_owned())?;
    writeln!(stream, "{}", command.line()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    Ok(reply.trim().to_owned())
}
