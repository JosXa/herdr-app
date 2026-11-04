use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Default)]
pub struct Config {
    pub interval: Option<Duration>,
    pub freeze: bool,
}

pub fn config() -> Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    *CONFIG.get_or_init(|| {
        let mode = std::env::var("HERDR_RESIZE_TRIAL").unwrap_or_else(|_| "off".into());
        assert!(matches!(mode.as_str(), "off" | "throttle" | "freeze" | "freeze-throttle"));
        let throttle = matches!(mode.as_str(), "throttle" | "freeze-throttle");
        let interval = throttle.then(|| {
            let hz: f64 = std::env::var("HERDR_RESIZE_HZ").unwrap_or_else(|_| "15".into()).parse().unwrap();
            assert!(hz.is_finite() && hz > 0.0);
            Duration::from_secs_f64(1.0 / hz)
        });
        Config { interval, freeze: matches!(mode.as_str(), "freeze" | "freeze-throttle") }
    })
}

pub struct Schedule {
    interval: Option<Duration>,
    last: Option<Instant>,
    pending: Option<(u16, u16)>,
}

impl Schedule {
    pub fn new(interval: Option<Duration>) -> Self { Self { interval, last: None, pending: None } }

    pub fn request(&mut self, target: (u16, u16), current: (u16, u16), now: Instant, immediate: bool) -> bool {
        if target == current && !immediate { self.pending = None; return false; }
        if immediate || self.interval.is_none() || self.last.is_none_or(|last| now.duration_since(last) >= self.interval.unwrap()) {
            self.pending = None;
            self.last = Some(now);
            true
        } else {
            self.pending = Some(target);
            false
        }
    }

    pub fn deadline(&self) -> Option<Instant> { self.pending.and(self.last.zip(self.interval).map(|(last, interval)| last + interval)) }
    pub fn cancel(&mut self) { self.pending = None; }
    pub fn take_due(&mut self, now: Instant) -> Option<(u16, u16)> {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.last = Some(now);
            self.pending.take()
        } else { None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ack { pub cols: u16, pub rows: u16, pub frame: u64 }

#[derive(Default)]
pub struct Gate {
    target: Option<(u16, u16)>,
    since: Option<Instant>,
    last_frame: u64,
    floor: u64,
}

impl Gate {
    pub fn begin(&mut self, target: (u16, u16), now: Instant) {
        self.target = Some(target);
        self.since.get_or_insert(now);
        self.floor = self.last_frame;
    }
    pub fn accept(&mut self, ack: Ack) -> bool {
        let newer = ack.frame > self.last_frame;
        self.last_frame = self.last_frame.max(ack.frame);
        if newer && ack.frame > self.floor && self.target == Some((ack.cols, ack.rows)) {
            self.cancel();
            true
        } else { false }
    }
    pub fn deadline(&self) -> Option<Instant> { self.since.map(|since| since + Duration::from_secs(1)) }
    pub fn pending(&self) -> bool { self.target.is_some() }
    pub fn last_frame(&self) -> u64 { self.last_frame }
    pub fn cancel(&mut self) { self.target = None; self.since = None; }
    pub fn expire(&mut self, now: Instant) { if self.deadline().is_some_and(|deadline| now >= deadline) { self.cancel(); } }
}

pub enum Part { Data(Vec<u8>), Ack(Ack) }

#[derive(Default)]
pub struct MarkerStream { pending: Vec<u8> }

impl MarkerStream {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Part> {
        const PREFIX: &[u8] = b"\x1b]777;herdr-frame;";
        let mut parts = Vec::new();
        let mut data = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            if self.pending.is_empty() && byte != 0x1b { data.push(byte); continue; }
            self.pending.push(byte);
            let prefix_len = self.pending.len().min(PREFIX.len());
            if self.pending[..prefix_len] != PREFIX[..prefix_len] || self.pending.len() > 96 {
                data.extend_from_slice(&self.pending);
                self.pending.clear();
            } else if self.pending.len() > PREFIX.len() && byte == 7 {
                let ack = std::str::from_utf8(&self.pending[PREFIX.len()..self.pending.len()-1]).ok().and_then(|text| {
                    let mut fields = text.split(';');
                    let cols = fields.next()?.parse::<u16>().ok().filter(|n| *n > 0)?;
                    let rows = fields.next()?.parse::<u16>().ok().filter(|n| *n > 0)?;
                    let frame = fields.next()?.parse().ok()?;
                    fields.next().is_none().then_some(Ack { cols, rows, frame })
                });
                if let Some(ack) = ack {
                    if !data.is_empty() { parts.push(Part::Data(std::mem::take(&mut data))); }
                    parts.push(Part::Ack(ack));
                } else { data.extend_from_slice(&self.pending); }
                self.pending.clear();
            }
        }
        if !data.is_empty() { parts.push(Part::Data(data)); }
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_trial_latest_target_flushes_without_output_and_can_cancel() {
        let now = Instant::now();
        let mut schedule = Schedule::new(Some(Duration::from_millis(100)));
        assert!(schedule.request((80, 30), (80, 24), now, false));
        assert!(!schedule.request((80, 40), (80, 30), now + Duration::from_millis(10), false));
        assert!(!schedule.request((80, 50), (80, 30), now + Duration::from_millis(90), false));
        assert_eq!(schedule.take_due(now + Duration::from_millis(99)), None);
        assert_eq!(schedule.take_due(now + Duration::from_millis(100)), Some((80, 50)));
        assert!(!schedule.request((80, 60), (80, 50), now + Duration::from_millis(105), false));
        assert!(!schedule.request((80, 50), (80, 50), now + Duration::from_millis(106), false));
        assert_eq!(schedule.deadline(), None);
        schedule.request((80, 60), (80, 50), now + Duration::from_millis(110), false);
        schedule.cancel();
        assert_eq!(schedule.take_due(now + Duration::from_secs(1)), None);
        assert!(schedule.request((80, 60), (80, 50), now + Duration::from_millis(120), true));
        let mut off = Schedule::new(None);
        assert!(off.request((80, 30), (80, 24), now, false));
        assert!(off.request((80, 31), (80, 30), now, false));
    }

    #[test]
    fn resize_trial_requires_new_matching_completion_and_has_bounded_hold() {
        let now = Instant::now();
        let mut gate = Gate::default();
        gate.accept(Ack { cols: 80, rows: 24, frame: 10 });
        gate.begin((80, 40), now);
        assert!(!gate.accept(Ack { cols: 80, rows: 24, frame: 11 }));
        assert!(!gate.accept(Ack { cols: 80, rows: 40, frame: 10 }));
        assert!(gate.accept(Ack { cols: 80, rows: 40, frame: 12 }));
        assert!(!gate.pending());
        gate.begin((80, 50), now);
        gate.begin((80, 60), now + Duration::from_millis(900));
        gate.expire(now + Duration::from_secs(1));
        assert!(!gate.pending());
    }

    #[test]
    fn resize_trial_marker_preserves_feed_order_across_every_boundary() {
        let bytes = b"before\x1b[?2026l\x1b]777;herdr-frame;80;40;12\x07after";
        for split in 0..=bytes.len() {
            let mut parser = MarkerStream::default();
            let parts = parser.feed(&bytes[..split]).into_iter().chain(parser.feed(&bytes[split..]));
            let mut output = Vec::new();
            let mut acks = 0;
            for part in parts {
                match part {
                    Part::Data(data) => output.extend(data),
                    Part::Ack(ack) => { assert_eq!(output, b"before\x1b[?2026l"); assert_eq!(ack, Ack { cols:80, rows:40, frame:12 }); acks += 1; }
                }
            }
            assert_eq!(acks, 1);
            assert_eq!(output, b"before\x1b[?2026lafter");
        }
    }
}
