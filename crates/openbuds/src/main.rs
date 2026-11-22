//! `openbuds`: earbud status, control and Waybar output.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use openbuds_core::bluez::{self, Bluez};
use openbuds_core::samsung::link::{Link, parse_mac};
use openbuds_core::samsung::proto::{NoiseMode, Raw, ids};
use openbuds_core::{Snapshot, collect};
use serde_json::json;

#[derive(Parser)]
#[command(name = "openbuds", version, about = "Earbud status and control for Linux")]
struct Cli {
    /// BlueZ address of the earbuds. Default: the connected audio device.
    #[arg(long, global = true, value_name = "MAC")]
    device: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the state of the earbuds and the phone.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print one JSON object for a Waybar custom module.
    Waybar,
    /// List the devices BlueZ knows.
    Devices,
    /// Connect the earbuds through BlueZ.
    Connect,
    /// Disconnect the earbuds through BlueZ.
    Disconnect,
    /// Set the noise control mode (Samsung Buds2 only).
    Anc {
        /// off, anc or ambient
        mode: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Status { json } => status(cli.device.as_deref(), json),
        Command::Waybar => waybar(cli.device.as_deref()),
        Command::Devices => devices(),
        Command::Connect => bluez_call(cli.device.as_deref(), true),
        Command::Disconnect => bluez_call(cli.device.as_deref(), false),
        Command::Anc { mode } => anc(cli.device.as_deref(), &mode),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("openbuds: {message}");
            ExitCode::FAILURE
        }
    }
}

fn read(device: Option<&str>) -> Result<Snapshot, String> {
    collect(device)
}

fn status(device: Option<&str>, json: bool) -> Result<(), String> {
    let snap = read(device)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&snap).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if snap.address.is_none() {
        println!("No earbuds found.");
        return Ok(());
    }
    println!("{}", snap.name.as_deref().unwrap_or("Earbuds"));
    println!("  connected: {}", yes_no(snap.connected));
    if let Some(v) = snap.battery_left {
        println!("  left:      {v}%");
    }
    if let Some(v) = snap.battery_right {
        println!("  right:     {v}%");
    }
    if let Some(v) = snap.battery_case {
        println!("  case:      {v}%");
    }
    if let Some(v) = snap.battery {
        println!("  battery:   {v}%");
    }
    if let Some(mode) = snap.noise_mode {
        println!("  noise:     {}", mode_name(mode));
    }
    if let Some(phone) = &snap.phone {
        let charging = if phone.charging { " (charging)" } else { "" };
        println!("  phone:     {} {}%{charging}", phone.name, phone.battery);
    }
    Ok(())
}

fn waybar(device: Option<&str>) -> Result<(), String> {
    let snap = read(device)?;
    let (text, tooltip, class) = waybar_parts(&snap);
    let out = json!({ "text": text, "tooltip": tooltip, "class": class });
    println!("{out}");
    Ok(())
}

/// Text, tooltip and CSS class for a Waybar module.
fn waybar_parts(snap: &Snapshot) -> (String, String, &'static str) {
    if snap.address.is_none() {
        return (String::new(), "No earbuds found".into(), "disconnected");
    }
    let name = snap.name.clone().unwrap_or_else(|| "Earbuds".into());
    let class = if snap.connected { "connected" } else { "disconnected" };

    let text = match (snap.battery_left, snap.battery_right, snap.battery) {
        (Some(l), Some(r), _) => format!("L {l}% R {r}%"),
        (_, _, Some(b)) => format!("{b}%"),
        _ => String::new(),
    };

    let mut lines = vec![name, format!("connected: {}", yes_no(snap.connected))];
    if let Some(v) = snap.battery_left {
        lines.push(format!("left: {v}%"));
    }
    if let Some(v) = snap.battery_right {
        lines.push(format!("right: {v}%"));
    }
    if let Some(v) = snap.battery_case {
        lines.push(format!("case: {v}%"));
    }
    if let Some(phone) = &snap.phone {
        lines.push(format!("phone {}: {}%", phone.name, phone.battery));
    }
    (text, lines.join("\n"), class)
}

fn devices() -> Result<(), String> {
    let list = bluez::known_devices().map_err(|e| format!("BlueZ: {e}"))?;
    for d in list {
        let mark = if d.connected { "*" } else { " " };
        let audio = if d.audio { "audio" } else { "-" };
        println!("{mark} {}  {audio:5}  {}", d.address, d.name);
    }
    Ok(())
}

fn bluez_call(device: Option<&str>, connect: bool) -> Result<(), String> {
    let address = target_address(device)?;
    let mut bz = Bluez::new(&address).map_err(|e| format!("BlueZ: {e}"))?;
    if connect {
        bz.connect()
    } else {
        bz.disconnect()
    }
}

fn anc(device: Option<&str>, mode: &str) -> Result<(), String> {
    let mode = match mode {
        "off" => NoiseMode::Off,
        "anc" => NoiseMode::Anc,
        "ambient" => NoiseMode::Ambient,
        other => return Err(format!("unknown mode `{other}`; use off, anc or ambient")),
    };
    let address = target_address(device)?;
    let mac = parse_mac(&address).ok_or("bad address")?;
    let mut link = Link::open(mac, None).map_err(|e| format!("SPP: {e}"))?;
    link.send(&Raw::byte(ids::NOISE_CONTROLS, mode.encode()))
        .map_err(|e| format!("SPP send: {e}"))?;
    println!("noise mode set: {}", mode_name(mode));
    Ok(())
}

/// Resolve the address to act on: the flag, else the selected device.
fn target_address(device: Option<&str>) -> Result<String, String> {
    if let Some(addr) = device {
        return Ok(addr.to_owned());
    }
    collect(None)?
        .address
        .ok_or_else(|| "no earbuds found; pass --device".to_owned())
}

fn yes_no(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}

fn mode_name(mode: NoiseMode) -> &'static str {
    match mode {
        NoiseMode::Off => "off",
        NoiseMode::Anc => "anc",
        NoiseMode::Ambient => "ambient",
    }
}

