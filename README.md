# openbuds

Earbud status and control for Linux. Works with any Bluetooth earbuds through
BlueZ. Samsung Galaxy Buds2 get per-earbud battery and noise control over the
Samsung SPP channel. A paired phone's battery appears when KDE Connect runs.

> **Status: version 0.1.** The command line tool and the Waybar output work.
> The tray icon and popup window are not built yet.

## Features

| Feature | Any BlueZ earbuds | Samsung Galaxy Buds2 | Needs KDE Connect |
|---|:---:|:---:|:---:|
| Connection state | yes | yes | no |
| Device list | yes | yes | no |
| Connect and disconnect | yes | yes | no |
| Battery (one level) | yes, when the device reports it | yes | no |
| Battery left, right and case | no | yes | no |
| Noise control (off, ANC, ambient) | no | yes | no |
| Phone battery and charging | no | no | yes |

Other Samsung models use the same vendor ID. openbuds decodes them with the
Buds2 layout. The output for those models is not tested. Test before you rely on it.

## Requirements

- Linux with BlueZ (`bluetoothd`) and a D-Bus system bus. This is the default on
  nearly every distribution.
- A Rust toolchain, version 1.85 or newer, to build from source.
- Optional: KDE Connect (`kdeconnectd`) for phone battery.

## Install

```bash
git clone https://github.com/Thejus-M/openbuds
cd openbuds
./install.sh
```

`install.sh` builds the release binary and copies it to `~/.local/bin/openbuds`.
Make sure `~/.local/bin` is in your `PATH`.

## Use

```bash
openbuds status            # text summary
openbuds status --json     # JSON for scripts
openbuds waybar            # one JSON line for a Waybar custom module
openbuds devices           # every device BlueZ knows
openbuds connect           # connect the earbuds through BlueZ
openbuds disconnect
openbuds anc ambient       # off | anc | ambient (Samsung SPP only)
openbuds --device AA:BB:CC:DD:EE:FF status   # pick a device by address
```

### Waybar

Add a custom module to `~/.config/waybar/config.jsonc`:

```jsonc
"custom/earbuds": {
    "exec": "openbuds waybar",
    "return-type": "json",
    "interval": 30,
    "on-click": "openbuds connect"
}
```

Then add `"custom/earbuds"` to a `modules-*` list in the same file.

### Hyprland key bind

```
bindd = SUPER, N, Cycle earbuds noise control, exec, openbuds anc anc
```

## How it works

- **BlueZ** (`org.bluez` on the system bus) gives the device list, the
  connection state, and the battery level. openbuds does not open a socket to
  an earbud that BlueZ does not show as connected. An open socket pages the
  earbuds and makes them connect.
- **Samsung SPP** opens an RFCOMM socket to the status channel (channel 20 on the
  Buds2 firmware seen so far). The socket reads framed messages. It decodes the
  extended status frame.
- **KDE Connect** (`org.kde.kdeconnect` on the session bus) gives the phone
  battery. If KDE Connect is not running, openbuds prints no phone line.

If the Samsung read fails, openbuds shows the BlueZ values. It does not stop.

## Protocol notes

- Frame layout: `0xFD | int16 LE header | msgId | payload | CRC16-CCITT LE | 0xDD`.
  `galaxy_buds_rs` provides the framing and the CRC.
- Message ids and payload layouts come from
  [GalaxyBudsClient](https://github.com/timschneeb/GalaxyBudsClient).
- The buds do not echo a setting change back to the session that sent it.
  openbuds reads the status again to confirm a change.
- Only one program can hold the SPP channel. Stop other Galaxy Buds apps
  before you run `openbuds anc`.

## Limits

- Other brands show one battery level only. BlueZ does not expose per-earbud
  values for them.
- Samsung models other than Buds2 are not verified.
- The tray icon, the popup window and the background daemon are planned, not built.

## Project layout

```
crates/openbuds-core/   BlueZ, Samsung SPP, KDE Connect, combined status
crates/openbuds/        command line tool
```

## Development

```bash
cargo build
cargo test
cargo clippy
```

## License

MIT. See [LICENSE](LICENSE).
