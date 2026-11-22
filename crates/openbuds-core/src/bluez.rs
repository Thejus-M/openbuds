//! The BlueZ gate.
//!
//! Connecting an RFCOMM socket pages the earbuds, so a daemon that simply
//! retries *creates* the Bluetooth link it is supposed to be observing: BlueZ
//! reports the buds as connected, no audio profile is negotiated behind it,
//! and the user is left with a panel that says "connected" over silence.
//!
//! budsd therefore follows BlueZ rather than leading it. It attaches to the
//! SPP channel only while the system already holds the device, and a
//! connection the user asks for is made by BlueZ, which brings A2DP and the
//! rest up with it.

use zbus::blocking::{Connection, Proxy};

const SERVICE: &str = "org.bluez";
const DEVICE_IFACE: &str = "org.bluez.Device1";

pub struct Bluez {
    conn: Connection,
    address: String,
    path: Option<String>,
}

impl Bluez {
    pub fn new(address: &str) -> zbus::Result<Bluez> {
        Ok(Bluez {
            conn: Connection::system()?,
            address: address.to_ascii_uppercase(),
            path: None,
        })
    }

    fn proxy(&self, path: &str) -> zbus::Result<Proxy<'_>> {
        Proxy::new(&self.conn, SERVICE, path.to_owned(), DEVICE_IFACE)
    }

    /// BlueZ names a device `/org/bluez/<adapter>/dev_AA_BB_…`. Walking the
    /// handful of possible adapters is cheaper and far less typing than a
    /// GetManagedObjects round trip, and it fails closed: a path that does
    /// not answer is not our device.
    fn find(&self) -> Option<String> {
        let suffix = self.address.replace(':', "_");
        for adapter in 0..4 {
            let path = format!("/org/bluez/hci{adapter}/dev_{suffix}");
            let answers = self
                .proxy(&path)
                .and_then(|p| p.get_property::<bool>("Connected"))
                .is_ok();
            if answers {
                return Some(path);
            }
        }
        None
    }

    /// The cached path, re-resolved when BlueZ stops answering on it (the
    /// adapter was reset, the device was removed and paired again).
    fn path(&mut self) -> Option<String> {
        if let Some(path) = self.path.clone() {
            let alive = self
                .proxy(&path)
                .and_then(|p| p.get_property::<bool>("Connected"))
                .is_ok();
            if alive {
                return Some(path);
            }
            self.path = None;
        }
        self.path = self.find();
        self.path.clone()
    }

    /// Is the device connected as far as the rest of the system is concerned?
    /// This is the same bit `bluetoothctl info` prints and the same one the
    /// audio stack acts on, which is the point: the widget must not claim a
    /// connection the user cannot hear.
    pub fn connected(&mut self) -> bool {
        let Some(path) = self.path() else {
            return false;
        };
        self.proxy(&path)
            .and_then(|p| p.get_property::<bool>("Connected"))
            .unwrap_or(false)
    }

    pub fn connect(&mut self) -> Result<(), String> {
        self.call("Connect")
    }

    pub fn disconnect(&mut self) -> Result<(), String> {
        self.call("Disconnect")
    }

    fn call(&mut self, method: &str) -> Result<(), String> {
        let path = self
            .path()
            .ok_or_else(|| format!("{} is not a device BlueZ knows", self.address))?;
        // BlueZ pages the buds here, which takes seconds; the default D-Bus
        // call timeout is the right ceiling for it.
        self.proxy(&path)
            .map_err(|e| e.to_string())?
            .call::<_, _, ()>(method, &())
            .map_err(|e| match e {
                zbus::Error::MethodError(_, Some(message), _) => message,
                other => other.to_string(),
            })
    }
}

/// One device BlueZ knows about, as the device list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub address: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    /// Advertised audio or headset service (A2DP sink, HFP or Headset).
    pub audio: bool,
    /// Bluetooth vendor is Samsung (Modalias vendor 0075).
    pub samsung: bool,
}

const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const BATTERY_IFACE: &str = "org.bluez.Battery1";
const AUDIO_UUIDS: &[&str] = &[
    "0000110b", // A2DP sink
    "0000111e", // Handsfree
    "00001108", // Headset
];

/// List every BlueZ device, with its connection and battery state.
pub fn known_devices() -> zbus::Result<Vec<Known>> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue};

    type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

    let conn = Connection::system()?;
    let manager = Proxy::new(&conn, SERVICE, "/", OBJECT_MANAGER)?;
    let objects: Objects = manager.call("GetManagedObjects", &())?;

    let mut out = Vec::new();
    for ifaces in objects.values() {
        let Some(dev) = ifaces.get(DEVICE_IFACE) else {
            continue;
        };
        let text = |key: &str| -> String {
            dev.get(key)
                .and_then(|v| String::try_from(v.clone()).ok())
                .unwrap_or_default()
        };
        let flag = |key: &str| -> bool {
            dev.get(key)
                .and_then(|v| bool::try_from(v.clone()).ok())
                .unwrap_or(false)
        };
        let uuids: Vec<String> = dev
            .get("UUIDs")
            .and_then(|v| Vec::<String>::try_from(v.clone()).ok())
            .unwrap_or_default();

        let battery = ifaces
            .get(BATTERY_IFACE)
            .and_then(|b| b.get("Percentage"))
            .and_then(|v| u8::try_from(v.clone()).ok());

        let name = {
            let alias = text("Alias");
            if alias.is_empty() { text("Name") } else { alias }
        };

        out.push(Known {
            address: text("Address"),
            name,
            connected: flag("Connected"),
            battery,
            audio: uuids
                .iter()
                .any(|u| AUDIO_UUIDS.iter().any(|a| u.starts_with(a))),
            samsung: text("Modalias").starts_with("bluetooth:v0075"),
        });
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
    Ok(out)
}

impl Bluez {
    /// The device alias as BlueZ shows it, or `None` if BlueZ does not know it.
    pub fn name(&mut self) -> Option<String> {
        let path = self.path()?;
        let proxy = self.proxy(&path).ok()?;
        proxy
            .get_property::<String>("Alias")
            .ok()
            .filter(|s| !s.is_empty())
    }

    /// The single battery level BlueZ reports (`org.bluez.Battery1`).
    pub fn battery(&mut self) -> Option<u8> {
        let path = self.path()?;
        let proxy = Proxy::new(&self.conn, SERVICE, path, BATTERY_IFACE).ok()?;
        proxy.get_property::<u8>("Percentage").ok()
    }
}

