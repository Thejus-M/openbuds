//! `openbudsd`: owns the earbud link and the Phone via PC rules.
//!
//! Threads poll BlueZ, the SPP channel, the phone and local media. Each sends
//! one message to the main loop. The main loop runs [`Route`], runs the
//! resulting actions on helper threads, and writes the state file.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openbuds_core::bluez::{self, Bluez, Known};
use openbuds_core::ipc::{self, Command, StateDoc};
use openbuds_core::kdeconnect;
use openbuds_core::mpris;
use openbuds_core::phone;
use openbuds_core::route::{Action, Event, Owner, InFlight, Route};
use openbuds_core::samsung::link::{Link, parse_mac};
use openbuds_core::samsung::proto::NoiseMode;
use openbuds_core::samsung::state::{BudsState, Placement};
use openbuds_core::sink;

/// Seconds between keepalive link attempts while the phone should stay linked.
const LINK_KEEPALIVE_SECS: f64 = 15.0;
/// Delay before dropping the phone link when buds leave the ears.
const UNLINK_DEBOUNCE: Duration = Duration::from_millis(200);
/// Boost level for the buds sink while the phone plays through the PC.
const PHONE_BOOST_PERCENT: u32 = 100;

enum Msg {
    /// The earbuds BlueZ selected, or `None` when none was found.
    Device(Option<Known>),
    /// A decoded SPP status update.
    Spp(BudsState),
    /// The SPP channel closed.
    SppEnded,
    /// Phone reachability, playback and battery from KDE Connect.
    Phone {
        id: Option<String>,
        playing: bool,
        battery: Option<kdeconnect::Phone>,
    },
    /// A KDE Connect call state.
    Call(String),
    /// Local media started or stopped.
    PcPlaying(bool),
    /// A control request from a client, with a channel for the reply line.
    Ipc(Command, Sender<String>),
    /// A connect or disconnect finished.
    ConnectDone(Result<(), String>),
    /// The phone audio link was attempted. `true` when the card is up.
    Linked(bool),
    /// Timer tick: keeps the route moving and the state fresh.
    Tick,
}

struct Options {
    device: String,
    phone_id: String,
    phone_bt: String,
    dry_run: bool,
    no_spp: bool,
}

struct Daemon {
    opts: Options,
    tx: Sender<Msg>,
    device: Option<Known>,
    spp: Option<BudsState>,
    spp_running: Arc<AtomicBool>,
    route: Route,
    phone_id: Option<String>,
    phone_playing: bool,
    phone_battery: Option<kdeconnect::Phone>,
    phone_bt: Option<String>,
    phone_audio_linked: bool,
    /// Shared with the unlink debounce thread: should the phone stay linked?
    want_link: Arc<AtomicBool>,
    last_link_at: f64,
    link_busy: Arc<AtomicBool>,
    call_watch_for: Option<String>,
    call_tx: Sender<String>,
    last_error: String,
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn main() {
    let opts = parse_args();
    if let Err(e) = run(opts) {
        eprintln!("openbudsd: {e}");
        std::process::exit(1);
    }
}

fn parse_args() -> Options {
    let mut opts = Options {
        device: String::new(),
        phone_id: String::new(),
        phone_bt: String::new(),
        dry_run: false,
        no_spp: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => opts.device = args.next().unwrap_or_default(),
            "--phone-id" => opts.phone_id = args.next().unwrap_or_default(),
            "--phone-bt" => opts.phone_bt = args.next().unwrap_or_default(),
            "--dry-run" => opts.dry_run = true,
            "--no-spp" => opts.no_spp = true,
            "-h" | "--help" => {
                println!("openbudsd [--device MAC] [--phone-id ID] [--phone-bt MAC] [--dry-run] [--no-spp]");
                std::process::exit(0);
            }
            other => {
                eprintln!("openbudsd: unknown option {other}");
                std::process::exit(2);
            }
        }
    }
    opts
}

fn run(opts: Options) -> Result<(), String> {
    let (tx, rx) = channel::<Msg>();
    let (call_tx, call_rx) = channel::<String>();

    // Forward call events into the main loop.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            for kind in call_rx {
                if tx.send(Msg::Call(kind)).is_err() {
                    break;
                }
            }
        });
    }

    let via_pc = read_switch();
    let phone_bt_setting = opts.phone_bt.clone();
    let mut daemon = Daemon {
        opts,
        tx: tx.clone(),
        device: None,
        spp: None,
        spp_running: Arc::new(AtomicBool::new(false)),
        route: Route::new(via_pc),
        phone_id: None,
        phone_playing: false,
        phone_battery: None,
        phone_bt: None,
        phone_audio_linked: false,
        want_link: Arc::new(AtomicBool::new(false)),
        last_link_at: 0.0,
        link_busy: Arc::new(AtomicBool::new(false)),
        call_watch_for: None,
        call_tx,
        last_error: String::new(),
    };
    if !phone_bt_setting.is_empty() {
        daemon.phone_bt = Some(phone_bt_setting.to_ascii_uppercase());
    }

    start_pollers(tx.clone());
    start_ipc(tx.clone())?;

    eprintln!("openbudsd: running (dry-run: {})", daemon.opts.dry_run);
    for msg in rx {
        daemon.handle(msg);
        daemon.write_state();
    }
    Ok(())
}

fn read_switch() -> bool {
    let path = ipc::state_dir().join("switch.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("viaPc").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

fn save_switch(on: bool) {
    let dir = ipc::state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("switch.json"), format!("{{\"viaPc\":{on}}}\n"));
}

/// Background pollers. Each sends to the main loop and never blocks it.
fn start_pollers(tx: Sender<Msg>) {
    // BlueZ: which earbuds, connected or not.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let setting = std::env::args().skip_while(|a| a != "--device").nth(1).unwrap_or_default();
            loop {
                let dev = bluez::known_devices().ok().and_then(|list| select_device(list, &setting));
                if tx.send(Msg::Device(dev)).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(1500));
            }
        });
    }
    // Local media.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut last = None;
            loop {
                let playing = mpris::local_playing();
                if last != Some(playing) {
                    last = Some(playing);
                    if tx.send(Msg::PcPlaying(playing)).is_err() {
                        break;
                    }
                }
                thread::sleep(Duration::from_secs(1));
            }
        });
    }
    // Phone: reachability, playback, battery.
    {
        let tx = tx.clone();
        let setting = std::env::args().skip_while(|a| a != "--phone-id").nth(1).unwrap_or_default();
        thread::spawn(move || loop {
            let id = kdeconnect::phone_id(&setting);
            let playing = id.as_deref().map(kdeconnect::phone_playing).unwrap_or(false);
            let battery = kdeconnect::phone();
            if tx.send(Msg::Phone { id, playing, battery }).is_err() {
                break;
            }
            thread::sleep(Duration::from_secs(2));
        });
    }
    // Tick.
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
        if tx.send(Msg::Tick).is_err() {
            break;
        }
    });
}

/// The earbuds to show: the setting, else a connected audio device, else
/// the first audio device.
fn select_device(list: Vec<Known>, setting: &str) -> Option<Known> {
    if !setting.is_empty() {
        return list.into_iter().find(|d| d.address.eq_ignore_ascii_case(setting));
    }
    let mut audio: Vec<Known> = list.into_iter().filter(|d| d.audio).collect();
    let pos = audio.iter().position(|d| d.connected).unwrap_or(0);
    if audio.is_empty() { None } else { Some(audio.swap_remove(pos)) }
}

fn start_ipc(tx: Sender<Msg>) -> Result<(), String> {
    let dir = ipc::runtime_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = ipc::socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = std::os::unix::net::UnixListener::bind(&path).map_err(|e| e.to_string())?;
    thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let tx = tx.clone();
            thread::spawn(move || serve(stream, tx));
        }
    });
    Ok(())
}

fn serve(stream: std::os::unix::net::UnixStream, tx: Sender<Msg>) {
    use std::io::{BufRead, BufReader, Write};
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut line = String::new();
    if BufReader::new(stream).read_line(&mut line).is_err() {
        return;
    }
    let reply = match Command::parse(line.trim()) {
        None => format!("error: unknown command: {}", line.trim()),
        Some(cmd) => {
            let (rtx, rrx) = channel::<String>();
            if tx.send(Msg::Ipc(cmd, rtx)).is_err() {
                return;
            }
            rrx.recv().unwrap_or_else(|_| "error: daemon stopped".into())
        }
    };
    let _ = writeln!(writer, "{reply}");
}

impl Daemon {
    fn connected(&self) -> bool {
        self.device.as_ref().map(|d| d.connected).unwrap_or(false)
    }

    fn address(&self) -> Option<String> {
        self.device.as_ref().map(|d| d.address.clone())
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Device(dev) => {
                self.device = dev;
                if !self.connected() {
                    self.spp = None;
                }
                self.start_spp_if_needed();
                self.buds_event(false);
            }
            Msg::Spp(state) => {
                let left_case = self.left_out_of_case(&state);
                self.spp = Some(state);
                self.buds_event(left_case);
            }
            Msg::SppEnded => {
                self.spp = None;
                self.buds_event(false);
            }
            Msg::Phone { id, playing, battery } => {
                let reachable = id.is_some();
                if id.is_some() && id != self.phone_id {
                    self.phone_id = id.clone();
                }
                self.phone_playing = playing;
                self.phone_battery = battery;
                if let Some(id) = id.clone()
                    && self.call_watch_for.as_deref() != Some(id.as_str())
                {
                    kdeconnect::watch_calls(id.clone(), self.call_tx.clone());
                    self.call_watch_for = Some(id);
                }
                if self.phone_bt.is_none() && self.route.via_pc {
                    self.phone_bt = phone::resolve("");
                }
                self.event(Event::PhoneReachable(reachable));
            }
            Msg::Call(kind) => self.event(Event::Call(kind)),
            Msg::PcPlaying(playing) => self.event(Event::PcPlaying(playing)),
            Msg::Ipc(cmd, reply) => {
                let result = self.control(cmd);
                let _ = reply.send(match result {
                    Ok(()) => "ok".into(),
                    Err(e) => {
                        self.last_error = e.clone();
                        format!("error: {e}")
                    }
                });
            }
            Msg::ConnectDone(result) => match result {
                Ok(()) => self.last_error.clear(),
                Err(e) => {
                    self.last_error = e;
                    self.event(Event::ConnectFailed);
                }
            },
            Msg::Linked(linked) => {
                self.phone_audio_linked = linked;
                if !linked {
                    self.run_restore_sink();
                }
            }
            Msg::Tick => {
                if self.route.via_pc && self.phone_bt.is_none() {
                    self.phone_bt = phone::resolve("");
                }
                if let Some(bt) = self.phone_bt.clone() {
                    self.phone_audio_linked = phone::audio_linked(&bt);
                }
                self.event(Event::Tick);
            }
        }
    }

    /// Start the SPP reader when the buds are connected and the model is Samsung.
    fn start_spp_if_needed(&mut self) {
        let Some(dev) = self.device.clone() else { return };
        if self.opts.no_spp || !dev.connected || !dev.samsung || self.spp_running.load(Ordering::SeqCst) {
            return;
        }
        let running = self.spp_running.clone();
        running.store(true, Ordering::SeqCst);
        let tx = self.tx.clone();
        thread::spawn(move || spp_reader(dev.address, tx, running));
    }

    fn left_out_of_case(&self, next: &BudsState) -> bool {
        let Some(prev) = &self.spp else { return false };
        (prev.placement_left == Placement::InCase && next.placement_left == Placement::Outside)
            || (prev.placement_right == Placement::InCase && next.placement_right == Placement::Outside)
    }

    /// Feed the route the buds state. `left_case` starts the prelink window.
    fn buds_event(&mut self, left_case: bool) {
        self.start_spp_if_needed();
        let worn = self.spp.as_ref().map(|s| s.wearing_left || s.wearing_right).unwrap_or(false);
        let connected = self.connected();
        self.event(Event::Buds { connected, worn, left_case });
    }

    fn event(&mut self, event: Event) {
        let actions = self.route.handle(event, now());
        self.want_link.store(self.route.should_link(now()), Ordering::SeqCst);
        for action in actions {
            self.run_action(action);
        }
    }

    fn control(&mut self, cmd: Command) -> Result<(), String> {
        match cmd {
            Command::ViaPc(on) => {
                save_switch(on);
                self.event(Event::SetViaPc(on));
            }
            Command::UseHere => self.event(Event::UseHere),
            Command::SendToPhone => self.event(Event::SendToPhone),
            Command::Connect => {
                let action = Action::Connect;
                self.run_action(action);
            }
            Command::Disconnect => {
                self.run_action(Action::Disconnect);
            }
        }
        Ok(())
    }

    fn run_action(&mut self, action: Action) {
        if self.opts.dry_run {
            eprintln!("openbudsd [dry-run]: {action:?}");
            return;
        }
        match action {
            Action::Connect => self.spawn_connect(None),
            Action::ScheduleConnect { ms } => self.spawn_connect(Some(ms)),
            Action::Disconnect => {
                let Some(addr) = self.address() else { return };
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let result = Bluez::new(&addr)
                        .map_err(|e| e.to_string())
                        .and_then(|mut b| b.disconnect());
                    let _ = tx.send(Msg::ConnectDone(result));
                });
            }
            Action::PingPhone(message) => {
                if let Some(id) = self.phone_id.clone() {
                    thread::spawn(move || kdeconnect::ping(&id, message));
                }
            }
            Action::LinkAudio => self.spawn_link(),
            Action::UnlinkAudio { immediate } => self.spawn_unlink(immediate),
        }
    }

    fn spawn_connect(&mut self, delay_ms: Option<u64>) {
        let Some(addr) = self.address() else {
            let _ = self.tx.send(Msg::ConnectDone(Err("no earbuds found".into())));
            return;
        };
        let tx = self.tx.clone();
        thread::spawn(move || {
            if let Some(ms) = delay_ms {
                thread::sleep(Duration::from_millis(ms));
            }
            let result = Bluez::new(&addr)
                .map_err(|e| e.to_string())
                .and_then(|mut b| b.connect());
            let _ = tx.send(Msg::ConnectDone(result));
        });
    }

    fn spawn_link(&mut self) {
        let Some(bt) = self.phone_bt.clone() else { return };
        // Skip while a link runs, or a fresh link is up (keepalive every 15 s).
        if self.link_busy.load(Ordering::SeqCst) {
            return;
        }
        let t = now();
        if self.phone_audio_linked && t - self.last_link_at < LINK_KEEPALIVE_SECS {
            return;
        }
        self.last_link_at = t;
        self.link_busy.store(true, Ordering::SeqCst);
        let busy = self.link_busy.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            if let Err(e) = phone::link(&bt) {
                eprintln!("openbudsd: phone link: {e}");
            }
            thread::sleep(Duration::from_millis(500));
            let linked = phone::audio_linked(&bt);
            if linked {
                sink::boost(PHONE_BOOST_PERCENT);
            }
            busy.store(false, Ordering::SeqCst);
            let _ = tx.send(Msg::Linked(linked));
        });
    }

    fn spawn_unlink(&mut self, immediate: bool) {
        let Some(bt) = self.phone_bt.clone() else { return };
        let want = self.want_link.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            if !immediate {
                thread::sleep(UNLINK_DEBOUNCE);
                // Buds went back in the ears during the debounce: keep the link.
                if want.load(Ordering::SeqCst) {
                    return;
                }
            }
            if let Err(e) = phone::unlink(&bt) {
                eprintln!("openbudsd: phone unlink: {e}");
            }
            sink::restore();
            let _ = tx.send(Msg::Linked(false));
        });
    }

    fn run_restore_sink(&self) {
        thread::spawn(sink::restore);
    }

    fn write_state(&self) {
        let mut doc = StateDoc {
            updated_at: now() as u64,
            connected: self.connected(),
            name: self.device.as_ref().map(|d| d.name.clone()),
            address: self.address(),
            source: "none".into(),
            battery: self.device.as_ref().and_then(|d| d.battery),
            via_pc: self.route.via_pc,
            owner: owner_name(self.route.owner).into(),
            in_flight: in_flight_name(self.route.in_flight).into(),
            phone_audio_linked: self.phone_audio_linked,
            last_error: self.last_error.clone(),
            phone_name: self.phone_battery.as_ref().map(|p| p.name.clone()),
            phone_battery: self.phone_battery.as_ref().map(|p| p.battery),
            phone_charging: self.phone_battery.as_ref().map(|p| p.charging).unwrap_or(false),
            ..StateDoc::default()
        };
        if let Some(dev) = &self.device {
            doc.source = if self.spp.is_some() { "samsung-spp".into() } else { "bluez".into() };
            doc.name = Some(dev.name.clone());
            doc.address = Some(dev.address.clone());
        }
        if let Some(s) = &self.spp {
            doc.battery_left = Some(s.battery_left);
            doc.battery_right = Some(s.battery_right);
            doc.battery_case = Some(s.battery_case);
            doc.charging = s.charging_left || s.charging_right;
            doc.worn_left = s.wearing_left;
            doc.worn_right = s.wearing_right;
            doc.noise_mode = Some(noise_name(s.noise_mode).into());
        }
        if let Err(e) = ipc::write_state(&doc) {
            eprintln!("openbudsd: state file: {e}");
        }
    }
}

fn owner_name(owner: Owner) -> &'static str {
    match owner {
        Owner::Pc => "pc",
        Owner::Call => "call",
        Owner::Sent => "sent",
        Owner::Taken => "taken",
    }
}

fn in_flight_name(flight: InFlight) -> &'static str {
    match flight {
        InFlight::None => "none",
        InFlight::Pc => "pc",
        InFlight::Phone => "phone",
    }
}

fn noise_name(mode: NoiseMode) -> &'static str {
    match mode {
        NoiseMode::Off => "off",
        NoiseMode::Anc => "anc",
        NoiseMode::Ambient => "ambient",
    }
}

/// SPP reader. Sends every change in the status, then `SppEnded` on close.
fn spp_reader(address: String, tx: Sender<Msg>, running: Arc<AtomicBool>) {
    let result = (|| -> Result<(), String> {
        let mac = parse_mac(&address).ok_or("bad address")?;
        let mut link = Link::open(mac, None).map_err(|e| e.to_string())?;
        let mut state = BudsState::default();
        loop {
            let frames = link.read_frames().map_err(|e| e.to_string())?;
            let mut changed = false;
            for frame in frames {
                changed |= state.apply(frame.id, &frame.payload);
            }
            if changed && tx.send(Msg::Spp(state.clone())).is_err() {
                return Ok(());
            }
        }
    })();
    if let Err(e) = result {
        eprintln!("openbudsd: SPP: {e}");
    }
    running.store(false, Ordering::SeqCst);
    let _ = tx.send(Msg::SppEnded);
}
