//! One combined view of the earbuds and the optional phone.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::bluez::{self, Bluez, Known};
use crate::kdeconnect::{self, Phone};
use crate::samsung::link::{Link, parse_mac};
use crate::samsung::proto::{NoiseMode, ids};
use crate::samsung::state::BudsState;

/// How long the SPP status read may take before the BlueZ view is used alone.
const SPP_TIMEOUT: Duration = Duration::from_secs(6);

/// Where the numbers in a [`Snapshot`] come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// No earbuds found.
    None,
    /// BlueZ only: one battery level at most.
    Bluez,
    /// BlueZ plus Samsung SPP: per-earbud battery and noise mode.
    SamsungSpp,
}

/// Everything one status read reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub source: Source,
    pub connected: bool,
    pub address: Option<String>,
    pub name: Option<String>,
    /// Single battery level from BlueZ (any brand).
    pub battery: Option<u8>,
    pub battery_left: Option<u8>,
    pub battery_right: Option<u8>,
    pub battery_case: Option<u8>,
    pub noise_mode: Option<NoiseMode>,
    pub phone: Option<Phone>,
}

impl Snapshot {
    fn empty() -> Snapshot {
        Snapshot {
            source: Source::None,
            connected: false,
            address: None,
            name: None,
            battery: None,
            battery_left: None,
            battery_right: None,
            battery_case: None,
            noise_mode: None,
            phone: None,
        }
    }
}

/// Pick the earbuds: the given address, else a connected audio device, else
/// the first audio device.
fn select(devices: &[Known], wanted: Option<&str>) -> Option<Known> {
    if let Some(addr) = wanted {
        return devices
            .iter()
            .find(|d| d.address.eq_ignore_ascii_case(addr))
            .cloned();
    }
    devices
        .iter()
        .find(|d| d.audio && d.connected)
        .or_else(|| devices.iter().find(|d| d.audio))
        .cloned()
}

/// Read the earbuds (and phone) state. `wanted` is a BlueZ address, or `None`
/// for automatic selection.
pub fn collect(wanted: Option<&str>) -> Result<Snapshot, String> {
    let devices = bluez::known_devices().map_err(|e| format!("BlueZ: {e}"))?;
    let mut snap = Snapshot::empty();
    snap.phone = kdeconnect::phone();

    let Some(dev) = select(&devices, wanted) else {
        return Ok(snap);
    };

    snap.address = Some(dev.address.clone());
    snap.name = Some(dev.name.clone());
    snap.connected = dev.connected;
    snap.source = Source::Bluez;
    snap.battery = dev.battery;

    if !dev.connected {
        return Ok(snap);
    }

    if dev.samsung {
        if let Some(buds) = read_spp(&dev.address) {
            snap.source = Source::SamsungSpp;
            snap.battery_left = Some(buds.battery_left);
            snap.battery_right = Some(buds.battery_right);
            snap.battery_case = Some(buds.battery_case);
            snap.noise_mode = Some(buds.noise_mode);
        }
    }

    if snap.battery.is_none() {
        snap.battery = Bluez::new(&dev.address)
            .ok()
            .and_then(|mut b| b.battery());
    }
    Ok(snap)
}

/// Open the SPP status channel and wait for the extended status frame.
fn read_spp(address: &str) -> Option<BudsState> {
    let mac = parse_mac(address)?;
    let mut link = Link::open(mac, None).ok()?;
    let deadline = Instant::now() + SPP_TIMEOUT;
    let mut state = BudsState::default();
    let mut got_status = false;

    while Instant::now() < deadline && !got_status {
        let frames = link.read_frames().ok()?;
        for frame in frames {
            got_status |= frame.id == ids::EXTENDED_STATUS_UPDATED;
            state.apply(frame.id, &frame.payload);
        }
    }
    got_status.then_some(state)
}
