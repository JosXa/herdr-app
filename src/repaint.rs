use std::time::{Duration, Instant};

pub type Token = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completion {
    pub token: Token,
    pub cols: u16,
    pub rows: u16,
    pub frame: u64,
}

#[derive(Default)]
pub struct Gate {
    token: Option<Token>,
    last_frame: Option<u64>,
    ended: bool,
    active: bool,
    since: Option<Instant>,
}

impl Gate {
    pub fn observe(&mut self, completion: Completion, current: (u16, u16)) -> bool {
        if self.token != Some(completion.token) {
            *self = Self { token: Some(completion.token), ..Self::default() };
        }
        if self.ended || self.last_frame.is_some_and(|last| completion.frame <= last) { return false; }
        self.last_frame = Some(completion.frame);
        if (completion.cols, completion.rows) != current { return false; }
        self.active = true;
        self.since = None;
        true
    }

    pub fn end(&mut self, token: Token) {
        if self.token == Some(token) {
            self.ended = true;
            self.deactivate();
        }
    }

    pub fn begin(&mut self, now: Instant) {
        if self.active {
            self.since.get_or_insert(now);
        }
    }

    pub fn active(&self) -> bool { self.active }
    pub fn pending(&self) -> bool { self.since.is_some() }
    pub fn deadline(&self) -> Option<Instant> { self.since.map(|since| since + Duration::from_secs(1)) }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.since = None;
    }

    pub fn expire(&mut self, now: Instant) {
        if self.deadline().is_some_and(|deadline| now >= deadline) { self.deactivate(); }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Part { Data(Vec<u8>), Frame(Completion), End(Token) }

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
                let marker = std::str::from_utf8(&self.pending[PREFIX.len()..self.pending.len()-1]).ok().and_then(|text| {
                    let mut fields = text.split(';');
                    let token = fields.next()?.as_bytes();
                    if token.len() != 32 || !token.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)) { return None; }
                    let token: Token = token.try_into().ok()?;
                    let value = fields.next()?;
                    if value == "end" { return fields.next().is_none().then_some(Part::End(token)); }
                    let cols = value.parse::<u16>().ok().filter(|n| *n > 0)?;
                    let rows = fields.next()?.parse::<u16>().ok().filter(|n| *n > 0)?;
                    let frame = fields.next()?.parse().ok()?;
                    fields.next().is_none().then_some(Part::Frame(Completion { token, cols, rows, frame }))
                });
                if let Some(marker) = marker {
                    if !data.is_empty() { parts.push(Part::Data(std::mem::take(&mut data))); }
                    parts.push(marker);
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

    const FIRST: Token = *b"0123456789abcdef0123456789abcdef";
    const SECOND: Token = *b"abcdef0123456789abcdef0123456789";

    #[test]
    fn only_matching_completed_frames_activate_and_release_retention() {
        let now = Instant::now();
        let mut gate = Gate::default();
        gate.begin(now);
        assert!(!gate.pending());
        assert!(!gate.observe(Completion { token: FIRST, cols: 80, rows: 20, frame: 0 }, (80, 24)));
        assert!(!gate.active());
        assert!(gate.observe(Completion { token: FIRST, cols: 80, rows: 24, frame: 1 }, (80, 24)));
        gate.begin(now);
        assert!(!gate.observe(Completion { token: FIRST, cols: 80, rows: 24, frame: 2 }, (80, 40)));
        assert!(gate.pending());
        assert!(!gate.observe(Completion { token: FIRST, cols: 80, rows: 40, frame: 1 }, (80, 40)));
        assert!(gate.pending());
        assert!(gate.observe(Completion { token: FIRST, cols: 80, rows: 40, frame: 3 }, (80, 40)));
        assert!(!gate.pending());
        gate.begin(now);
        gate.begin(now + Duration::from_millis(900));
        gate.expire(now + Duration::from_secs(1));
        assert!(!gate.active());
        gate.begin(now + Duration::from_secs(2));
        assert!(!gate.pending());
        assert!(gate.observe(Completion { token: FIRST, cols: 80, rows: 70, frame: 4 }, (80, 70)));
    }

    #[test]
    fn renderer_restart_resets_sequence_and_old_disposal_cannot_end_it() {
        let mut gate = Gate::default();
        assert!(gate.observe(Completion { token: FIRST, cols: 80, rows: 24, frame: 900 }, (80, 24)));
        gate.end(FIRST);
        assert!(!gate.active());
        assert!(!gate.observe(Completion { token: FIRST, cols: 80, rows: 24, frame: 901 }, (80, 24)));
        assert!(gate.observe(Completion { token: SECOND, cols: 80, rows: 24, frame: 0 }, (80, 24)));
        gate.end(FIRST);
        assert!(gate.active());
        gate.begin(Instant::now());
        assert!(gate.pending());
        assert!(!gate.observe(Completion { token: *b"11111111111111111111111111111111", cols: 80, rows: 24, frame: 0 }, (80, 40)));
        assert!(!gate.active());
        assert!(!gate.pending());
    }

    #[test]
    fn marker_boundaries_preserve_frame_order_and_unrecognized_bytes() {
        let bytes = b"before\x1b[?2026l\x1b]777;herdr-frame;0123456789abcdef0123456789abcdef;80;40;12\x07after\x1b]777;herdr-frame;0123456789abcdef0123456789abcdef;end\x07";
        for split in 0..=bytes.len() {
            let mut parser = MarkerStream::default();
            let parts: Vec<_> = parser.feed(&bytes[..split]).into_iter().chain(parser.feed(&bytes[split..])).collect();
            let mut output = Vec::new();
            let mut markers = Vec::new();
            for part in parts {
                match part {
                    Part::Data(data) => output.extend(data),
                    Part::Frame(completion) => { assert_eq!(output, b"before\x1b[?2026l"); markers.push(Part::Frame(completion)); }
                    Part::End(token) => { assert_eq!(output, b"before\x1b[?2026lafter"); markers.push(Part::End(token)); }
                }
            }
            assert_eq!(markers, vec![Part::Frame(Completion { token: FIRST, cols: 80, rows: 40, frame: 12 }), Part::End(FIRST)]);
        }
        for bytes in [b"\x1b]777;herdr-frame;80;40;12\x07".as_slice(), b"\x1b]777;herdr-frame;bad;end\x07", b"\x1b[31mordinary\x1b[0m"] {
            let mut parser = MarkerStream::default();
            assert_eq!(parser.feed(bytes), vec![Part::Data(bytes.to_vec())]);
        }
    }
}
