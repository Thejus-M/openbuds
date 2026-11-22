//! Optional KDE Connect integration over the session bus.
//!
//! Reads the battery of the first reachable phone that reports one. When
//! KDE Connect is not running, every function returns `None`. Nothing fails.

use zbus::blocking::{Connection, Proxy};

const SERVICE: &str = "org.kde.kdeconnect";
const DAEMON_PATH: &str = "/modules/kdeconnect";
const DAEMON_IFACE: &str = "org.kde.kdeconnect.daemon";
const DEVICE_IFACE: &str = "org.kde.kdeconnect.device";
const BATTERY_IFACE: &str = "org.kde.kdeconnect.device.battery";

/// A phone reached through KDE Connect.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Phone {
    pub name: String,
    pub battery: u8,
    pub charging: bool,
}

/// Return the first reachable phone with a battery, or `None`.
pub fn phone() -> Option<Phone> {
    let conn = Connection::session().ok()?;
    let daemon = Proxy::new(&conn, SERVICE, DAEMON_PATH, DAEMON_IFACE).ok()?;
    let ids: Vec<String> = daemon.call("devices", &()).ok()?;

    ids.iter().find_map(|id| read_phone(&conn, id))
}

fn read_phone(conn: &Connection, id: &str) -> Option<Phone> {
    let base = format!("/modules/kdeconnect/devices/{id}");

    let device = Proxy::new(conn, SERVICE, base.clone(), DEVICE_IFACE).ok()?;
    if !device.get_property::<bool>("isReachable").ok()? {
        return None;
    }
    let name: String = device.get_property("name").ok()?;

    let battery_path = format!("{base}/battery");
    let battery = Proxy::new(conn, SERVICE, battery_path, BATTERY_IFACE).ok()?;
    if !battery.get_property::<bool>("hasBattery").ok()? {
        return None;
    }
    let charge: i32 = battery.get_property("charge").ok()?;
    let charging: bool = battery.get_property("isCharging").ok()?;

    Some(Phone {
        name,
        battery: charge.clamp(0, 100) as u8,
        charging,
    })
}

/// The phone's KDE Connect id: the setting if set, else the first reachable
/// device. `None` when KDE Connect is off or no phone answers.
pub fn phone_id(setting: &str) -> Option<String> {
    if !setting.is_empty() {
        return Some(setting.to_owned());
    }
    let conn = Connection::session().ok()?;
    let daemon = Proxy::new(&conn, SERVICE, DAEMON_PATH, DAEMON_IFACE).ok()?;
    let ids: Vec<String> = daemon.call("devices", &()).ok()?;
    ids.into_iter().find(|id| {
        Proxy::new(&conn, SERVICE, format!("/modules/kdeconnect/devices/{id}"), DEVICE_IFACE)
            .and_then(|d| d.get_property::<bool>("isReachable"))
            .unwrap_or(false)
    })
}

/// Is the phone playing media (through KDE Connect's MPRIS proxy)?
pub fn phone_playing(id: &str) -> bool {
    let Ok(conn) = Connection::session() else {
        return false;
    };
    Proxy::new(
        &conn,
        SERVICE,
        format!("/modules/kdeconnect/devices/{id}/mprisremote"),
        "org.kde.kdeconnect.device.mprisremote",
    )
    .and_then(|p| p.get_property::<bool>("isPlaying"))
    .unwrap_or(false)
}

/// Send a KDE Connect ping. The phone's Samsung routine reads the message.
pub fn ping(id: &str, message: &str) {
    let _ = std::process::Command::new("kdeconnect-cli")
        .args(["-d", id, "--ping-msg", message])
        .status();
}

/// Watch the phone's telephony signals on a thread. Sends each call state
/// (`ringing`, `talking`, `missedCall`) on `tx`. Restarts when the monitor exits.
pub fn watch_calls(id: String, tx: std::sync::mpsc::Sender<String>) {
    std::thread::spawn(move || loop {
        let path = format!("/modules/kdeconnect/devices/{id}/telephony");
        let child = std::process::Command::new("gdbus")
            .args(["monitor", "--session", "--dest", SERVICE, "--object-path", &path])
            .stdout(std::process::Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        };
        if let Some(out) = child.stdout.take() {
            use std::io::BufRead;
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(rest) = line.split("callReceived ('").nth(1) {
                    let kind = rest.split('\'').next().unwrap_or("").to_owned();
                    if tx.send(kind).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
}
