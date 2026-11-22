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
