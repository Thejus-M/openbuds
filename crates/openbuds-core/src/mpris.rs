//! Is media playing on this PC? Phone players proxied by KDE Connect and
//! playerctld mirrors do not count.

use zbus::blocking::{Connection, Proxy};

const PREFIX: &str = "org.mpris.MediaPlayer2.";

/// True when any local MPRIS player reports `Playing`.
pub fn local_playing() -> bool {
    let Ok(conn) = Connection::session() else {
        return false;
    };
    let Ok(dbus) = Proxy::new(&conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus") else {
        return false;
    };
    let Ok(names): Result<Vec<String>, _> = dbus.call("ListNames", &()) else {
        return false;
    };
    names
        .iter()
        .filter(|n| n.starts_with(PREFIX))
        .filter(|n| !n.contains("playerctld") && !n.to_lowercase().contains("kdeconnect"))
        .any(|name| {
            Proxy::new(&conn, name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player")
                .and_then(|p| p.get_property::<String>("PlaybackStatus"))
                .map(|status| status == "Playing")
                .unwrap_or(false)
        })
}
