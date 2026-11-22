//! Phone via PC: the rules that decide when the earbuds link to the phone.
//!
//! A port of the routing logic in the old home.buds `Panel.qml`. This module
//! has no I/O. It takes events and returns actions. The daemon runs the
//! actions. Tests run without hardware.
//!
//! Model: the buds stay on this PC. The phone plays into the PC as a Bluetooth
//! speaker (A2DP source). A call moves the buds to the phone until the PC plays
//! something again.

/// Seconds the phone link stays up after a bud leaves the case.
pub const PRELINK_SECS: f64 = 5.0;
/// Seconds a manual choice ("Use here", "Send to phone") beats the rules.
pub const HOLD_SECS: f64 = 120.0;
/// Connect attempts before the route pauses.
pub const MAX_CONNECT_RETRIES: u8 = 5;
/// Pause after the last failed connect, seconds.
pub const RETRY_PAUSE_SECS: f64 = 10.0;
/// Seconds a call-start ignores new PC audio, so the call audio does not undo it.
pub const CALL_GRACE_SECS: f64 = 5.0;

/// Why the phone holds the buds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Nobody claims the buds; the PC has them.
    Pc,
    /// The phone is in a call.
    Call,
    /// The user sent them to the phone.
    Sent,
    /// The buds left without a command from us.
    Taken,
}

/// Which switch is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFlight {
    None,
    /// Moving the buds to this PC.
    Pc,
    /// Moving the buds to the phone.
    Phone,
}

/// What the daemon sees now. Sent to [`Route::handle`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Buds connection or wear state changed.
    Buds {
        connected: bool,
        worn: bool,
        /// A bud just left the case. Starts the prelink window.
        left_case: bool,
    },
    /// Local (non-phone) audio started or stopped on this PC.
    PcPlaying(bool),
    /// KDE Connect reports the phone reachable or not.
    PhoneReachable(bool),
    /// KDE Connect telephony: `ringing`, `talking`, `missedCall`.
    Call(String),
    /// User chose "Use here".
    UseHere,
    /// User chose "Send to phone".
    SendToPhone,
    /// User switched Phone via PC on or off.
    SetViaPc(bool),
    /// A connect to this PC failed.
    ConnectFailed,
    /// Periodic tick: keeps the phone link alive and retries the route.
    Tick,
}

/// Work for the daemon to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Ask BlueZ to connect the buds.
    Connect,
    /// Ask BlueZ to disconnect the buds.
    Disconnect,
    /// Retry the connect after `ms` milliseconds.
    ScheduleConnect { ms: u64 },
    /// Send a KDE Connect ping. Phone follows the message (`buds:take` or `buds:release`).
    PingPhone(&'static str),
    /// Link the phone's audio profile now.
    LinkAudio,
    /// Drop the phone's audio profile. `immediate` skips the 0.2 s debounce.
    UnlinkAudio { immediate: bool },
}

/// The whole Phone via PC state.
#[derive(Debug, Clone)]
pub struct Route {
    pub via_pc: bool,
    pub owner: Owner,
    pub in_flight: InFlight,
    pub connected: bool,
    pub worn: bool,
    pub pc_playing: bool,
    pub phone_reachable: bool,
    pub in_call: bool,
    call_since: f64,
    retries: u8,
    retry_pause_until: f64,
    hold_until: f64,
    prelink_until: f64,
    link_wanted: bool,
}

impl Route {
    pub fn new(via_pc: bool) -> Route {
        Route {
            via_pc,
            owner: Owner::Pc,
            in_flight: InFlight::None,
            connected: false,
            worn: false,
            pc_playing: false,
            phone_reachable: false,
            in_call: false,
            call_since: 0.0,
            retries: 0,
            retry_pause_until: 0.0,
            hold_until: 0.0,
            prelink_until: 0.0,
            link_wanted: false,
        }
    }

    /// Phone link should be up: Phone via PC on, buds here and worn, or about to be.
    pub fn should_link(&self, now: f64) -> bool {
        self.via_pc && self.connected && (self.worn || now < self.prelink_until)
    }

    /// Apply one event at time `now` (seconds). Returns the actions to run.
    pub fn handle(&mut self, event: Event, now: f64) -> Vec<Action> {
        let mut out = Vec::new();
        match event {
            Event::Buds { connected, worn, left_case } => {
                let was_connected = self.connected;
                self.worn = worn;
                if left_case {
                    self.prelink_until = now + PRELINK_SECS;
                }
                self.connected = connected;
                if connected != was_connected {
                    self.on_connected_change(connected);
                }
            }
            Event::PcPlaying(playing) => {
                self.pc_playing = playing;
                if playing {
                    let call_recent = self.in_call && now - self.call_since < CALL_GRACE_SECS;
                    if self.owner != Owner::Pc && !call_recent {
                        self.clear_owner();
                    }
                }
            }
            Event::PhoneReachable(reachable) => self.phone_reachable = reachable,
            Event::Call(kind) => {
                if kind == "ringing" || kind == "talking" {
                    self.on_call(now, &mut out);
                }
            }
            Event::UseHere => {
                self.clear_owner();
                self.hold_until = now + HOLD_SECS;
                self.retry_pause_until = 0.0;
                self.take_buds(&mut out);
            }
            Event::SendToPhone => {
                self.hold_until = now + HOLD_SECS;
                self.owner = Owner::Sent;
                self.release_buds(&mut out);
            }
            Event::SetViaPc(on) => {
                self.via_pc = on;
                if on {
                    self.clear_owner();
                    self.in_flight = InFlight::None;
                } else {
                    self.in_flight = InFlight::None;
                }
            }
            Event::ConnectFailed => self.on_connect_failed(now, &mut out),
            Event::Tick => {}
        }
        self.evaluate(now, &mut out);
        self.sync_link(now, &mut out);
        out
    }

    fn on_connected_change(&mut self, connected: bool) {
        if !connected && self.in_flight == InFlight::None && self.owner == Owner::Pc {
            // Buds left without our command: the phone took them. Do not grab back.
            self.owner = Owner::Taken;
        }
        if !connected && self.in_flight == InFlight::Pc {
            // A connect was in flight; the failure path decides the retry.
        }
        self.in_flight = InFlight::None;
        if connected {
            self.retries = 0;
            self.owner = Owner::Pc;
        }
    }

    fn on_call(&mut self, now: f64, out: &mut Vec<Action>) {
        if self.in_call && self.owner == Owner::Call {
            return;
        }
        self.in_call = true;
        self.call_since = now;
        self.owner = Owner::Call;
        if self.connected || self.in_flight == InFlight::Pc {
            self.release_buds(out);
        } else {
            out.push(Action::PingPhone("buds:take"));
        }
    }

    fn clear_owner(&mut self) {
        self.owner = Owner::Pc;
        self.in_call = false;
        self.hold_until = 0.0;
    }

    fn take_buds(&mut self, out: &mut Vec<Action>) {
        self.in_flight = InFlight::Pc;
        self.retries = 0;
        if self.phone_reachable {
            out.push(Action::PingPhone("buds:release"));
        }
        out.push(Action::Connect);
    }

    fn release_buds(&mut self, out: &mut Vec<Action>) {
        self.in_flight = InFlight::Phone;
        if self.connected {
            out.push(Action::Disconnect);
        }
        out.push(Action::PingPhone("buds:take"));
    }

    fn on_connect_failed(&mut self, now: f64, out: &mut Vec<Action>) {
        if self.in_flight != InFlight::Pc || self.connected {
            return;
        }
        if self.retries >= MAX_CONNECT_RETRIES {
            self.in_flight = InFlight::None;
            self.retry_pause_until = now + RETRY_PAUSE_SECS;
            return;
        }
        self.retries += 1;
        out.push(Action::ScheduleConnect { ms: 700 });
    }

    /// The rule: buds live on this PC unless the phone owns them for now.
    fn evaluate(&mut self, now: f64, out: &mut Vec<Action>) {
        if !self.via_pc {
            return;
        }
        if self.connected || self.owner != Owner::Pc || self.in_flight == InFlight::Pc {
            return;
        }
        if now < self.retry_pause_until {
            return;
        }
        self.take_buds(out);
    }

    /// Link or unlink the phone's audio when the wanted state changes.
    fn sync_link(&mut self, now: f64, out: &mut Vec<Action>) {
        let wanted = self.should_link(now);
        if wanted {
            out.push(Action::LinkAudio);
        } else if self.link_wanted {
            // Buds left the PC: drop at once. Buds out of the ears: 0.2 s debounce.
            let immediate = self.via_pc && !self.connected;
            out.push(Action::UnlinkAudio { immediate });
        }
        self.link_wanted = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buds(connected: bool, worn: bool) -> Event {
        Event::Buds { connected, worn, left_case: false }
    }

    #[test]
    fn via_pc_on_takes_buds_when_disconnected() {
        let mut r = Route::new(false);
        let out = r.handle(Event::SetViaPc(true), 0.0);
        assert!(out.contains(&Action::Connect));
        assert_eq!(r.in_flight, InFlight::Pc);
    }

    #[test]
    fn worn_bud_links_phone_audio() {
        let mut r = Route::new(true);
        r.handle(buds(true, false), 0.0);
        let out = r.handle(buds(true, true), 1.0);
        assert!(out.contains(&Action::LinkAudio));
    }

    #[test]
    fn prelink_window_after_leaving_case() {
        let mut r = Route::new(true);
        r.handle(buds(true, false), 0.0);
        let out = r.handle(Event::Buds { connected: true, worn: false, left_case: true }, 10.0);
        assert!(out.contains(&Action::LinkAudio));
        // Five seconds later, not worn, prelink over: unlink with debounce.
        let later = r.handle(Event::Tick, 16.0);
        assert!(later.contains(&Action::UnlinkAudio { immediate: false }));
    }

    #[test]
    fn buds_leaving_pc_unlinks_at_once() {
        let mut r = Route::new(true);
        r.handle(buds(true, true), 0.0);
        let out = r.handle(buds(false, false), 1.0);
        assert!(out.contains(&Action::UnlinkAudio { immediate: true }));
    }

    #[test]
    fn call_moves_buds_to_phone_and_pc_audio_takes_them_back() {
        let mut r = Route::new(true);
        r.handle(buds(true, true), 0.0);
        let out = r.handle(Event::Call("ringing".into()), 100.0);
        assert!(out.contains(&Action::Disconnect));
        assert!(out.contains(&Action::PingPhone("buds:take")));
        // The daemon reports the buds gone after our disconnect.
        r.handle(buds(false, false), 101.0);

                assert_eq!(r.owner, Owner::Call);

        // Call audio on the PC within the grace period does not clear it.
        r.handle(Event::PcPlaying(true), 102.0);
        assert_eq!(r.owner, Owner::Call);

        // Later PC audio clears it; the route then asks for the buds back.
        r.handle(Event::PcPlaying(false), 200.0);
        let out = r.handle(Event::PcPlaying(true), 210.0);
        assert_eq!(r.owner, Owner::Pc);
        assert!(out.contains(&Action::Connect));
    }

    #[test]
    fn buds_taken_away_are_not_grabbed_back() {
        let mut r = Route::new(true);
        r.handle(buds(true, true), 0.0);
        let out = r.handle(buds(false, false), 5.0);
        assert_eq!(r.owner, Owner::Taken);
        assert!(!out.contains(&Action::Connect));
    }

    #[test]
    fn use_here_holds_against_the_rules() {
        let mut r = Route::new(true);
        r.handle(Event::Call("talking".into()), 0.0);
        let out = r.handle(Event::UseHere, 50.0);
        assert!(out.contains(&Action::Connect));
        assert_eq!(r.owner, Owner::Pc);
    }

    #[test]
    fn repeated_connect_failures_pause_the_route() {
        let mut r = Route::new(true);
        r.handle(Event::UseHere, 0.0);
        for _ in 0..MAX_CONNECT_RETRIES {
            let out = r.handle(Event::ConnectFailed, 1.0);
            assert!(out.contains(&Action::ScheduleConnect { ms: 700 }));
        }
        // Next failure gives up and pauses.
        let out = r.handle(Event::ConnectFailed, 1.0);
        assert!(out.is_empty());
        assert_eq!(r.in_flight, InFlight::None);
        // While paused, the rule does not reconnect.
        let out = r.handle(Event::Tick, 5.0);
        assert!(!out.contains(&Action::Connect));
    }

    #[test]
    fn via_pc_off_unlinks_nothing_when_not_linked() {
        let mut r = Route::new(true);
        let out = r.handle(Event::SetViaPc(false), 0.0);
        assert!(!out.iter().any(|a| matches!(a, Action::LinkAudio)));
    }
}
