//! Samsung Galaxy Buds protocol over RFCOMM (SPP).
//!
//! Framing and CRC come from `galaxy_buds_rs`. The payload layouts are
//! mirrored from GalaxyBudsClient and decoded as Buds2. Other Samsung models
//! may send a different layout; read them with care.

pub mod link;
pub mod proto;
pub mod state;
