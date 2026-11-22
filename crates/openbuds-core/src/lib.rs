//! Earbud status and control for Linux.
//!
//! Three sources feed one [`Snapshot`]:
//! - BlueZ (any brand): connection state, device name and the single battery
//!   level that BlueZ exposes.
//! - Samsung SPP (Galaxy Buds2 only in this version): per-earbud battery and
//!   noise control.
//! - KDE Connect (optional): battery of a paired phone.
//!
//! [`route`] holds the Phone via PC rules. It has no I/O.

pub mod battery;
pub mod bluez;
pub mod kdeconnect;
pub mod route;
pub mod samsung;
pub mod status;

pub use status::{Snapshot, Source, collect};
