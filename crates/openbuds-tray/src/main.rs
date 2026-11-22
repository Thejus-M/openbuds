//! `openbuds-tray`: a tray icon with the lowest earbud battery drawn on it.
//!
//! The menu sends commands to `openbudsd` over its socket. The icon and the
//! tooltip read the daemon's state file once a second.

use std::thread;
use std::time::Duration;

use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};
use ksni::{Icon, ToolTip, Tray, TrayService};
use openbuds_core::battery;
use openbuds_core::ipc::{self, Command, StateDoc};

const SIZE: i32 = 22;

/// 3x5 pixel digits, one row per byte, 3 bits used, left bit first.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];
const DASH: [u8; 5] = [0b000, 0b000, 0b111, 0b000, 0b000];

struct AppTray {
    doc: StateDoc,
}

/// The number on the icon: the lowest earbud level, else the BlueZ level.
fn bar_label(doc: &StateDoc) -> Option<String> {
    if doc.source == "samsung-spp" {
        let left = doc.battery_left.unwrap_or(0);
        let right = doc.battery_right.unwrap_or(0);
        return battery::bar_text(left, right, doc.charging);
    }
    doc.battery.filter(|&v| v > 0).map(|v| format!("{v}%"))
}

/// Draw a text label as an ARGB icon. Only digits and a dash are available.
fn render(label: Option<&str>, connected: bool) -> Icon {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    // Rounded background: dark, mostly opaque.
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 - 10.5).abs();
            let dy = (y as f32 - 10.5).abs();
            let corner = dx.max(0.0) > 8.5 && dy.max(0.0) > 8.5
                && (dx - 8.5).powi(2) + (dy - 8.5).powi(2) > 4.0;
            if !corner && dx <= 10.5 && dy <= 10.5 {
                set(&mut px, x, y, [0xC0, 0x18, 0x18, 0x18]);
            }
        }
    }
    let ink = if connected { [0xFF, 0xFF, 0xFF, 0xFF] } else { [0xFF, 0x80, 0x80, 0x80] };
    let glyphs: Vec<[u8; 5]> = match label {
        None => vec![DASH],
        Some(text) => text
            .chars()
            .filter_map(|c| match c {
                '0'..='9' => Some(DIGITS[c as usize - '0' as usize]),
                '—' | '-' => Some(DASH),
                _ => None,
            })
            .collect(),
    };
    // Each glyph is 3x5 at scale 2, with 1 px gaps. Center the row.
    let scale = 2;
    let width = glyphs.len() as i32 * (3 * scale) + (glyphs.len() as i32 - 1).max(0) * scale;
    let height = 5 * scale;
    let x0 = (SIZE - width) / 2;
    let y0 = (SIZE - height) / 2;
    for (i, glyph) in glyphs.iter().enumerate() {
        let gx = x0 + i as i32 * (3 * scale + scale);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            set(&mut px, gx + col * scale + sx, y0 + row as i32 * scale + sy, ink);
                        }
                    }
                }
            }
        }
    }
    Icon { width: SIZE, height: SIZE, data: px }
}

/// Write one pixel. ARGB, network byte order.
fn set(px: &mut [u8], x: i32, y: i32, argb: [u8; 4]) {
    if !(0..SIZE).contains(&x) || !(0..SIZE).contains(&y) {
        return;
    }
    let i = ((y * SIZE + x) * 4) as usize;
    px[i..i + 4].copy_from_slice(&argb);
}

fn owner_text(doc: &StateDoc) -> String {
    match doc.owner.as_str() {
        "call" => "Phone is in a call".into(),
        "sent" => "Sent to phone".into(),
        "taken" => "Phone took the buds".into(),
        _ => "On this PC".into(),
    }
}

fn send(cmd: Command) {
    if let Err(e) = ipc::send(cmd) {
        eprintln!("openbuds-tray: {e}");
    }
}

impl Tray for AppTray {
    fn id(&self) -> String {
        "openbuds".into()
    }

    fn title(&self) -> String {
        self.doc.name.clone().unwrap_or_else(|| "Earbuds".into())
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let label = bar_label(&self.doc);
        vec![render(label.as_deref(), self.doc.connected)]
    }

    fn tool_tip(&self) -> ToolTip {
        let mut lines = Vec::new();
        if self.doc.connected {
            lines.push("connected".to_owned());
        } else {
            lines.push("disconnected".to_owned());
        }
        if let Some(v) = self.doc.battery_left {
            lines.push(format!("left: {}", battery::level_text(v)));
        }
        if let Some(v) = self.doc.battery_right {
            lines.push(format!("right: {}", battery::level_text(v)));
        }
        if let Some(v) = self.doc.battery_case {
            lines.push(format!("case: {}", battery::level_text(v)));
        }
        if let Some(v) = self.doc.battery {
            lines.push(format!("battery: {v}%"));
        }
        if let Some(name) = &self.doc.phone_name {
            let battery = self.doc.phone_battery.map(|v| format!(" {v}%")).unwrap_or_default();
            lines.push(format!("phone {name}:{battery}"));
        }
        if self.doc.via_pc {
            lines.push(format!("Phone via PC · {}", owner_text(&self.doc)));
        }
        ToolTip {
            icon_name: String::new(),
            icon_pixmap: vec![],
            title: self.title(),
            description: lines.join("\n"),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = vec![
            CheckmarkItem {
                label: "Phone via PC".into(),
                enabled: true,
                visible: true,
                checked: self.doc.via_pc,
                icon_name: String::new(),
                icon_data: Vec::new(),
                shortcut: Vec::new(),
                disposition: ksni::menu::Disposition::Normal,
                activate: Box::new(|t: &mut Self| send(Command::ViaPc(!t.doc.via_pc))),
            }
            .into(),
            StandardItem {
                label: "Use here".into(),
                activate: Box::new(|_| send(Command::UseHere)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Send to phone".into(),
                activate: Box::new(|_| send(Command::SendToPhone)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Connect".into(),
                activate: Box::new(|_| send(Command::Connect)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Disconnect".into(),
                activate: Box::new(|_| send(Command::Disconnect)),
                ..Default::default()
            }
            .into(),
        ];
        if !self.doc.last_error.is_empty() {
            items.push(MenuItem::Separator);
            items.push(
                StandardItem {
                    label: format!("Error: {}", self.doc.last_error),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

fn main() {
    let service = TrayService::new(AppTray {
        doc: ipc::read_state().unwrap_or_default(),
    });
    let handle = service.handle();
    service.spawn();

    // Follow the daemon's state file.
    loop {
        let doc = ipc::read_state().unwrap_or_default();
        handle.update(|t| t.doc = doc.clone());
        thread::sleep(Duration::from_secs(1));
    }
}
