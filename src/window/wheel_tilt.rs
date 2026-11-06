use gpui::{ScrollDelta, ScrollWheelEvent};
use std::time::{Duration, Instant};

const BURST_GAP: Duration = Duration::from_millis(80);
const HOLD_REPEAT: Duration = Duration::from_millis(350);

#[derive(Default)]
pub(super) struct WheelTilt {
    burst: Option<Burst>,
}

struct Burst {
    forward: bool,
    last_event: Instant,
    last_step: Instant,
}

pub(super) fn tilt_forward(event: &ScrollWheelEvent) -> Option<bool> {
    let ScrollDelta::Lines(delta) = event.delta else { return None };
    if event.modifiers.shift || delta.y != 0.0 || delta.x == 0.0 {
        return None;
    }
    Some(delta.x < 0.0)
}

impl WheelTilt {
    pub(super) fn step(&mut self, forward: bool, now: Instant) -> bool {
        if let Some(burst) = self.burst.as_mut().filter(|burst| burst.forward == forward && now.duration_since(burst.last_event) < BURST_GAP) {
            burst.last_event = now;
            if now.duration_since(burst.last_step) < HOLD_REPEAT {
                return false;
            }
            burst.last_step = now;
            return true;
        }
        self.burst = Some(Burst { forward, last_event: now, last_step: now });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, ScrollWheelEvent, point, px};

    fn wheel(delta: ScrollDelta, shift: bool) -> ScrollWheelEvent {
        ScrollWheelEvent { delta, modifiers: Modifiers { shift, ..Default::default() }, ..Default::default() }
    }

    #[test]
    fn only_a_plain_horizontal_wheel_tilts() {
        assert_eq!(tilt_forward(&wheel(ScrollDelta::Lines(point(-3., 0.)), false)), Some(true));
        assert_eq!(tilt_forward(&wheel(ScrollDelta::Lines(point(3., 0.)), false)), Some(false));
        assert_eq!(tilt_forward(&wheel(ScrollDelta::Lines(point(-3., 0.)), true)), None);
        assert_eq!(tilt_forward(&wheel(ScrollDelta::Lines(point(0., 3.)), false)), None);
        assert_eq!(tilt_forward(&wheel(ScrollDelta::Pixels(point(px(-30.), px(0.))), false)), None);
    }

    #[test]
    fn a_tilt_burst_steps_once_and_repeats_while_held() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut tilt = WheelTilt::default();
        let quick: Vec<bool> = (0..8).map(|i| tilt.step(true, at(i * 20))).collect();
        assert_eq!(quick, [true, false, false, false, false, false, false, false]);
        assert!(tilt.step(true, at(480)));
        assert!(tilt.step(false, at(500)));
        let held: Vec<u64> = (1..40).map(|i| 500 + i * 20).filter(|&ms| tilt.step(false, at(ms))).collect();
        assert_eq!(held, [860, 1220]);
    }
}
