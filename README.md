# openbuds

Earbud status and control for Linux. Works with any Bluetooth earbuds
through BlueZ. Samsung Galaxy Buds2 also get per-earbud battery, noise
control and Phone via PC. A paired phone's battery appears when KDE
Connect runs.

> **Status: early development.** The CLI, the daemon and the tray build and
> pass their unit tests. Phone via PC and the tray were run once on real
> hardware. A phone audio link problem is open (see Known issues).

## Components

| Binary | What it does |
|---|---|
| `openbuds` | Command line tool: status, device list, connect, disconnect, Waybar JSON, noise mode, daemon commands. |
| `openbudsd` | Daemon. Holds the single SPP link to the buds. Runs the Phone via PC rules. Writes the state file. |
| `openbuds-tray` | Tray icon. Shows the lowest earbud battery as a number on the icon. Menu for Phone via PC and connection. |

## Features

| Feature | Any BlueZ earbuds | Samsung Galaxy Buds2 | Needs KDE Connect |
|---|:---:|:---:|:---:|
| Connection state | yes | yes | no |
| Device list | yes | yes | no |
| Connect and disconnect | yes | yes | no |
| Battery (one level) | yes, when the device reports it | yes | no |
| Battery left, right and case | no | yes | no |
| Noise control (off, ANC, ambient) | no | yes | no |
| Phone via PC | no | yes | yes |
| Phone battery and charging | no | no | yes |

Other Samsung models share the vendor ID. openbuds decodes them with the
Buds2 layout. Their numbers are not verified.

## Requirements

- Linux with BlueZ (`bluetoothd`) and the system D-Bus.
- PipeWire or PulseAudio with `pactl`, for Phone via PC.
- Optional: KDE Connect (`kdeconnectd`), for the phone battery and calls.
- Rust 1.88 or newer, to build.

## Install

```bash
git clone https://github.com/Thejus-M/openbuds
cd openbuds
./install.sh
```

`install.sh` builds the three binaries into `~/.local/bin`. It links the
user unit `openbudsd.service` and the tray autostart entry. It starts
nothing. Start the daemon with:

```bash
systemctl --user start openbudsd
```

## Use

```bash
openbuds status              # text summary
openbuds status --json       # JSON
openbuds waybar              # one JSON line for a Waybar custom module
openbuds devices             # every device BlueZ knows
openbuds connect             # connect through BlueZ
openbuds disconnect
openbuds anc ambient         # off | anc | ambient (Samsung SPP only)
openbuds --device AA:BB:CC:DD:EE:FF status    # pick a device by address
```

The tray icon shows the lowest earbud battery. Its menu runs Phone via PC,
Use here, Send to phone, Connect and Disconnect.

### Phone via PC

The buds stay on this PC. The phone plays into this PC as a Bluetooth
speaker, so every phone app reaches the buds. A call moves the buds to the
phone. Local audio on the PC moves them back.

- A worn bud links the phone audio. Taking the buds out of the ears drops the link after 0.2 s.
- While linked, the buds sink is raised to 100%. The old level is restored on unlink.
- "Use here" and "Send to phone" hold their choice for two minutes.
- Five failed connects in a row pause the route for ten seconds.

The phone must offer an A2DP source. The daemon finds it among the paired
phones, or uses `--phone-bt`.

### Swap from the old Galaxy Buds stack

`contrib/swap-to-rust.sh` stops `galaxybuds`, removes the old bar widget
and starts `openbudsd` and the tray. It keeps a backup of the bar layout.
`contrib/swap-back.sh` restores everything. Only one stack can hold the
buds' SPP link at a time.

## How it works

- **BlueZ** (`org.bluez`, system bus) gives the device list, the connection
  state and the battery. openbuds opens no SPP socket while BlueZ shows
  the buds disconnected, because a socket pages the buds.
- **Samsung SPP** opens an RFCOMM socket to the status channel (channel 20 on
  the Buds2 firmware seen so far). It decodes the extended status frame.
- **KDE Connect** (`org.kde.kdeconnect`, session bus) gives the phone battery,
  its playback and call events. Without KDE Connect, no phone line appears.
- **MPRIS** (session bus) tells whether local media plays. KDE Connect's
  proxied players do not count.
- **State file** `$XDG_RUNTIME_DIR/openbuds/state.json` is written by the
  daemon after each change. The tray and the CLI read it.
- **Socket** `$XDG_RUNTIME_DIR/openbuds/ipc.sock` takes one command per line:
  `via-pc on|off`, `use-here`, `send-to-phone`, `connect`, `disconnect`.

## Protocol notes

- Frame layout: `0xFD | int16 LE header | msgId | payload | CRC16-CCITT LE | 0xDD`.
  The `galaxy_buds_rs` crate provides framing and the CRC.
- Message ids and payload layouts come from
  [GalaxyBudsClient](https://github.com/timschneeb/GalaxyBudsClient).
- The buds do not echo a setting change. openbuds reads the status again.

## Known issues

- The phone audio link fails with `br-connection-busy` while a bud is worn.
  The daemon retries once a second. Cause not found yet.
- The tray icon image is not checked on a real display.
- Other Samsung models are not verified.
- Non-Samsung buds show one battery level. They have no wear-based linking.

## Project layout

```
crates/openbuds-core/     BlueZ, Samsung SPP, KDE Connect, MPRIS, phone link, sink boost, state, rules
crates/openbuds/          command line tool
crates/openbuds-daemon/   openbudsd
crates/openbuds-tray/     tray icon
contrib/                  user unit, tray autostart, swap scripts
```

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets
```

## License

MIT. See [LICENSE](LICENSE).
