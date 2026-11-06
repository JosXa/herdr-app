//! Windows: overlays a mod opens with `kind: "window"`. They stay until
//! closed, move where they're dropped (a press anywhere the mod doesn't keep
//! for itself drags), and the one pressed last is on top. With a `key` a
//! window remembers its spot, and opening that key again replaces it; with
//! `keep` it also reopens on the next start, in the layout that opened it.
//! A layout switch closes them (they belong to their layout, like its mods).
//! With `resizable` its edges and corners resize it, and a key remembers the size too.

use super::*;

/// How far the pointer moves before a press becomes a drag.
const DRAG_START: f32 = 4.0;
/// How much of a window stays on screen however far it's dragged, px.
const KEEP_SHOWN: f32 = 48.0;
/// How far a resizable window's grips reach into and beyond its edge, px.
const GRIP: f32 = 5.0;
/// The smallest a resize leaves a window, px at zoom 1.
const MIN_WIDTH: f32 = 240.0;
const MIN_HEIGHT: f32 = 160.0;

pub struct WinDrag {
    pub id: String,
    /// The pointer's offset from the window's top left.
    grab: Point<Pixels>,
    down: Point<Pixels>,
    pointer: Point<Pixels>,
    moving: bool,
}

pub struct WinResize {
    pub id: String,
    /// Which edges follow the pointer: -1 the left or top, 1 the right or bottom.
    sx: i8,
    sy: i8,
    down: Point<Pixels>,
    start: Bounds<Pixels>,
    moved: bool,
}

// ---- windows.json: spots by key, and the kept windows of each layout

fn path() -> PathBuf {
    file("windows.json")
}

fn read() -> Value {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn edit(f: impl FnOnce(&mut Value)) {
    let mut v = read();
    f(&mut v);
    if let Ok(s) = serde_json::to_string_pretty(&v) {
        let _ = crate::persist::write_atomic(&path(), s + "\n");
    }
}

pub fn place_of(key: &str) -> Option<Point<Pixels>> {
    let p = read().get("places")?.get(key)?.as_array()?.clone();
    Some(point(px(p.first()?.as_f64()? as f32), px(p.get(1)?.as_f64()? as f32)))
}

fn remember(key: &str, at: Point<Pixels>) {
    edit(|v| v["places"][key] = json!([f32::from(at.x).round(), f32::from(at.y).round()]));
}

/// A resizable window's size by key, px at zoom 1.
pub fn size_of(key: &str) -> Option<Size<f32>> {
    let s = read().get("sizes")?.get(key)?.as_array()?.clone();
    Some(size(s.first()?.as_f64()? as f32, s.get(1)?.as_f64()? as f32))
}

fn remember_size(key: &str, s: Size<f32>) {
    edit(|v| v["sizes"][key] = json!([s.width.round(), s.height.round()]));
}

pub fn keep(layout: &str, key: &str, spec: &Value) {
    edit(|v| v["kept"][layout][key] = spec.clone());
}

pub fn forget(layout: &str, key: &str) {
    if read().get("kept").and_then(|k| k.get(layout)).and_then(|l| l.get(key)).is_none() {
        return;
    }
    edit(|v| {
        if let Some(l) = v["kept"].get_mut(layout).and_then(Value::as_object_mut) {
            l.remove(key);
        }
    });
}

fn kept(layout: &str) -> Vec<Value> {
    read().get("kept").and_then(|k| k.get(layout)).and_then(Value::as_object).map(|l| l.values().cloned().collect()).unwrap_or_default()
}

impl AppWindow {
    /// The windows a layout keeps, reopened (at start, or on switching to it).
    pub(super) fn open_kept_windows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for spec in kept(&self.layout_name) {
            self.open_overlay(mods::new_overlay_id(), spec, window, cx);
        }
    }

    /// Leaving a layout: its windows close (the kept ones come back with it).
    pub(super) fn close_windows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The layout's windows go with it; the app's own (What's new) stay.
        let ids: Vec<String> = self.overlays.iter().filter(|o| o.kind == OverlayKind::Window && !o.frame).map(|o| o.id.clone()).collect();
        for id in ids {
            self.remove_overlay(&id, window, cx);
        }
    }

    /// A window at its spot: where it was dropped, else centered once its size
    /// is known (it stays invisible until then), always partly on screen.
    pub(super) fn render_window(&self, o: &Overlay, body: Stateful<Div>, viewport: Size<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let measured = self.bounds.borrow().get(&o.slot).map(|b| b.size);
        let size = measured.unwrap_or_default();
        let at = match (&self.win_drag, o.place) {
            (Some(d), _) if d.id == o.id && d.moving => d.pointer - d.grab,
            (_, Some(p)) => p,
            _ => point((viewport.width - size.width) / 2.0, (viewport.height - size.height) / 2.0),
        };
        let keep = px(KEEP_SHOWN);
        let x = at.x.min(viewport.width - keep).max(keep - size.width);
        let y = at.y.min(viewport.height - keep).max(px(0.0));
        let measure = {
            let (bounds, slot, known) = (self.bounds.clone(), o.slot.clone(), measured.is_some());
            canvas(
                move |b, window, _| {
                    if !known {
                        window.request_animation_frame();
                    }
                    bounds.borrow_mut().insert(slot, b);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };
        let (raise, press) = (o.id.clone(), o.id.clone());
        let body = body
            .relative()
            .child(measure)
            // Pressed anywhere, even on the mod's own controls: to the front.
            .capture_any_mouse_down(cx.listener(move |this, _, _, cx| this.raise_window(&raise, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.window_press(&press, ev.position, cx);
                }),
            );
        let grips = if o.resizable { self.resize_grips(&o.id, cx) } else { Vec::new() };
        div().absolute().left(x).top(y).when(measured.is_none(), |d| d.opacity(0.0)).child(body).children(grips).into_any_element()
    }

    fn resize_grips(&self, id: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let g = px(GRIP);
        let grip = |sx: i8, sy: i8, cursor: CursorStyle| {
            let id = id.to_owned();
            div()
                .id(SharedString::from(format!("rz-{id}-{sx}-{sy}")))
                .absolute()
                .occlude()
                .cursor(cursor)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.resize_press(&id, sx, sy, ev.position, cx);
                    }),
                )
        };
        let corner = |sx: i8, sy: i8, cursor| {
            let d = grip(sx, sy, cursor).w(g * 3.).h(g * 3.);
            let d = if sx < 0 { d.left(-g) } else { d.right(-g) };
            if sy < 0 { d.top(-g) } else { d.bottom(-g) }
        };
        vec![
            grip(-1, 0, CursorStyle::ResizeLeftRight).left(-g).top(g * 2.).bottom(g * 2.).w(g * 2.).into_any_element(),
            grip(1, 0, CursorStyle::ResizeLeftRight).right(-g).top(g * 2.).bottom(g * 2.).w(g * 2.).into_any_element(),
            grip(0, -1, CursorStyle::ResizeUpDown).top(-g).left(g * 2.).right(g * 2.).h(g * 2.).into_any_element(),
            grip(0, 1, CursorStyle::ResizeUpDown).bottom(-g).left(g * 2.).right(g * 2.).h(g * 2.).into_any_element(),
            corner(-1, -1, CursorStyle::ResizeUpLeftDownRight).into_any_element(),
            corner(1, 1, CursorStyle::ResizeUpLeftDownRight).into_any_element(),
            corner(1, -1, CursorStyle::ResizeUpRightDownLeft).into_any_element(),
            corner(-1, 1, CursorStyle::ResizeUpRightDownLeft).into_any_element(),
        ]
    }

    fn resize_press(&mut self, id: &str, sx: i8, sy: i8, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(start) = self.bounds.borrow().get(&format!("overlay:{id}")).copied() else { return };
        self.raise_window(id, cx);
        self.win_resize = Some(WinResize { id: id.to_owned(), sx, sy, down: at, start, moved: false });
        cx.notify();
    }

    fn resize_move(&mut self, at: Point<Pixels>, pressed: bool, cx: &mut Context<Self>) {
        if !pressed {
            self.resize_release(cx);
            return;
        }
        let Some(r) = &mut self.win_resize else { return };
        r.moved = true;
        let (min_w, min_h) = (settings::z(MIN_WIDTH), settings::z(MIN_HEIGHT));
        let (d, b) = (at - r.down, r.start);
        let span = |s: i8, from: Pixels, length: Pixels, delta: Pixels, min: Pixels| match s {
            1 => (from, (length + delta).max(min)),
            -1 => {
                let l = (length - delta).max(min);
                (from + length - l, l)
            }
            _ => (from, length),
        };
        let (x, w) = span(r.sx, b.origin.x, b.size.width, d.x, min_w);
        let (y, h) = span(r.sy, b.origin.y, b.size.height, d.y, min_h);
        let id = r.id.clone();
        let zoom = settings::zoom();
        if let Some(o) = self.overlays.iter_mut().find(|o| o.id == id) {
            o.place = Some(point(x, y));
            o.size = size(f32::from(w) / zoom, f32::from(h) / zoom);
            o.fit = false;
        }
        cx.notify();
    }

    /// Ends a resize: the window keeps its new bounds (remembered under its key);
    /// returns true so the release doesn't also click what's under it.
    fn resize_release(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(r) = self.win_resize.take() else { return false };
        cx.notify();
        if let Some(o) = self.overlays.iter().find(|o| o.id == r.id)
            && let Some(k) = &o.key
            && r.moved
        {
            if let Some(p) = o.place {
                remember(k, p);
            }
            remember_size(k, o.size);
        }
        true
    }

    fn raise_window(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(i) = self.overlays.iter().position(|o| o.id == id) else { return };
        if i + 1 < self.overlays.len() {
            let o = self.overlays.remove(i);
            self.overlays.push(o);
            cx.notify();
        }
    }

    fn window_press(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        // A terminal in the window keeps its presses (selecting text).
        let on_terminal = self.overlays.iter().find(|o| o.id == id).and_then(|o| o.layout.as_ref()).is_some_and(|l| {
            let bounds = self.bounds.borrow();
            l.visible_blocks().iter().any(|n| matches!(n.content(), Some(Content::Terminal { .. })) && bounds.get(&n.id).is_some_and(|b| b.contains(&at)))
        });
        if on_terminal {
            return;
        }
        let Some(b) = self.bounds.borrow().get(&format!("overlay:{id}")).copied() else { return };
        self.win_drag = Some(WinDrag { id: id.to_owned(), grab: at - b.origin, down: at, pointer: at, moving: false });
        cx.notify();
    }

    fn window_move(&mut self, at: Point<Pixels>, pressed: bool, cx: &mut Context<Self>) {
        let Some(d) = &mut self.win_drag else { return };
        if !pressed {
            self.win_drag = None;
            cx.notify();
            return;
        }
        d.pointer = at;
        let dist = f32::from((at.x - d.down.x).abs()).max(f32::from((at.y - d.down.y).abs()));
        if d.moving || dist >= DRAG_START {
            d.moving = true;
            cx.notify();
        }
    }

    /// Ends a press: a drag leaves the window there (and remembers it under its
    /// key); returns true so the release doesn't also click what's under it.
    fn window_release(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(d) = self.win_drag.take() else { return false };
        cx.notify();
        if !d.moving {
            return false;
        }
        let at = d.pointer - d.grab;
        if let Some(o) = self.overlays.iter_mut().find(|o| o.id == d.id) {
            o.place = Some(at);
            if let Some(k) = &o.key {
                remember(k, at);
            }
        }
        true
    }

    /// While a window is pressed: follows the pointer anywhere in the app.
    pub(super) fn window_tracker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.win_drag.is_none() && self.win_resize.is_none() {
            return None;
        }
        let this = cx.entity().downgrade();
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let t = this.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let pressed = ev.pressed_button == Some(MouseButton::Left);
                            let _ = t.update(cx, |this, cx| {
                                this.window_move(ev.position, pressed, cx);
                                this.resize_move(ev.position, pressed, cx);
                            });
                        }
                    });
                    let t = this.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && ev.button == MouseButton::Left && t.update(cx, |this, cx| this.window_release(cx) | this.resize_release(cx)).unwrap_or(false) {
                            cx.stop_propagation();
                        }
                    });
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }
}
