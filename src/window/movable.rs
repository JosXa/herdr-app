//! Windows: overlays a mod opens with `kind: "window"`. They stay until
//! closed, move where they're dropped (a press anywhere the mod doesn't keep
//! for itself drags), and the one pressed last is on top. With a `key` a
//! window remembers its spot, and opening that key again replaces it; with
//! `keep` it also reopens on the next start, in the layout that opened it.
//! A layout switch closes them (they belong to their layout, like its mods).
//! With `resizable` its edges and corners resize it, and so does Shift + right drag anywhere on it
//! (the corner of the quarter pressed follows the pointer, the opposite one stays: Hyprland's
//! floating resize); a key remembers the size too;
//! one without a height follows its content's height until a top or bottom drag sets one
//! (a key remembers only its width).
//! With `pane` it lives inside that pane: drawn only while the pane is shown,
//! kept wholly inside it below its header a margin from its edges, and docked to the corner it
//! was left nearest.

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
/// How far a pane window stays from its pane's edges, px at zoom 1.
const PANE_MARGIN: f32 = 10.0;

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
    button: MouseButton,
    down: Point<Pixels>,
    start: Bounds<Pixels>,
    moved: bool,
}

/// A pane window's spot: its gap to the pane corner it sits nearest, so it
/// stays in that corner however the pane is resized.
#[derive(Clone, Copy, PartialEq)]
pub struct Dock {
    gap: Point<Pixels>,
    right: bool,
    bottom: bool,
}

impl Dock {
    fn of(window: Bounds<Pixels>, area: Bounds<Pixels>) -> Dock {
        Dock::anchored(window, area, window.center().x > area.center().x, window.center().y > area.center().y)
    }

    fn anchored(window: Bounds<Pixels>, area: Bounds<Pixels>, right: bool, bottom: bool) -> Dock {
        let gap_x = if right { area.right() - window.right() } else { window.left() - area.left() };
        let gap_y = if bottom { area.bottom() - window.bottom() } else { window.top() - area.top() };
        Dock { gap: point(gap_x.max(px(0.0)), gap_y.max(px(0.0))), right, bottom }
    }

    fn at(self, area: Bounds<Pixels>, s: Size<Pixels>) -> Point<Pixels> {
        point(
            if self.right { area.right() - s.width - self.gap.x } else { area.left() + self.gap.x },
            if self.bottom { area.bottom() - s.height - self.gap.y } else { area.top() + self.gap.y },
        )
    }
}

fn resize_cursor(sx: i8, sy: i8) -> CursorStyle {
    match (sx, sy) {
        (0, _) => CursorStyle::ResizeUpDown,
        (_, 0) => CursorStyle::ResizeLeftRight,
        _ if sx == sy => CursorStyle::ResizeUpLeftDownRight,
        _ => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// Where a window of that size lands wholly inside the area (its top left
/// when it's bigger).
fn inside(at: Point<Pixels>, s: Size<Pixels>, area: Bounds<Pixels>) -> (Pixels, Pixels) {
    (at.x.min(area.right() - s.width).max(area.left()), at.y.min(area.bottom() - s.height).max(area.top()))
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

pub fn dock_of(key: &str) -> Option<Dock> {
    let d = read().get("docks")?.get(key)?.clone();
    let n = |k: &str| d.get(k).and_then(Value::as_f64).map(|v| px(v as f32));
    Some(Dock { gap: point(n("x")?, n("y")?), right: d.get("right")?.as_bool()?, bottom: d.get("bottom")?.as_bool()? })
}

fn remember_dock(key: &str, d: Dock) {
    edit(|v| v["docks"][key] = json!({"x": f32::from(d.gap.x).round(), "y": f32::from(d.gap.y).round(), "right": d.right, "bottom": d.bottom}));
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
    pub(super) fn render_window(&self, o: &Overlay, body: Stateful<Div>, viewport: Size<Pixels>, pane: Option<Bounds<Pixels>>, cx: &mut Context<Self>) -> AnyElement {
        let measured = self.bounds.borrow().get(&o.slot).map(|b| b.size);
        let size = measured.unwrap_or_default();
        let area = pane.unwrap_or(Bounds { origin: Point::default(), size: viewport });
        let at = match (&self.win_drag, o.place, o.dock.filter(|_| pane.is_some())) {
            (Some(d), _, _) if d.id == o.id && d.moving => d.pointer - d.grab,
            (_, _, Some(dock)) => dock.at(area, size),
            (_, Some(p), _) if pane.is_none() => p,
            _ => area.origin + point((area.size.width - size.width) / 2.0, (area.size.height - size.height) / 2.0),
        };
        let (x, y) = match pane {
            Some(_) => inside(at, size, area),
            None => {
                let keep = px(KEEP_SHOWN);
                (at.x.min(viewport.width - keep).max(keep - size.width), at.y.min(viewport.height - keep).max(px(0.0)))
            }
        };
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
        let (raise, press, resizable) = (o.id.clone(), o.id.clone(), o.resizable);
        let body = body
            .relative()
            .child(measure)
            // Pressed anywhere, even on the mod's own controls: to the front.
            .capture_any_mouse_down(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                this.raise_window(&raise, cx);
                if resizable && ev.button == MouseButton::Right && ev.modifiers.shift {
                    cx.stop_propagation();
                    this.resize_by_quarter(&raise, ev.position, cx);
                }
            }))
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
        let grip = |sx: i8, sy: i8| {
            let id = id.to_owned();
            div()
                .id(SharedString::from(format!("rz-{id}-{sx}-{sy}")))
                .absolute()
                .occlude()
                .cursor(resize_cursor(sx, sy))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.resize_press(&id, sx, sy, MouseButton::Left, ev.position, cx);
                    }),
                )
        };
        let corner = |sx: i8, sy: i8| {
            let d = grip(sx, sy).w(g * 3.).h(g * 3.);
            let d = if sx < 0 { d.left(-g) } else { d.right(-g) };
            if sy < 0 { d.top(-g) } else { d.bottom(-g) }
        };
        vec![
            grip(-1, 0).left(-g).top(g * 2.).bottom(g * 2.).w(g * 2.).into_any_element(),
            grip(1, 0).right(-g).top(g * 2.).bottom(g * 2.).w(g * 2.).into_any_element(),
            grip(0, -1).top(-g).left(g * 2.).right(g * 2.).h(g * 2.).into_any_element(),
            grip(0, 1).bottom(-g).left(g * 2.).right(g * 2.).h(g * 2.).into_any_element(),
            corner(-1, -1).into_any_element(),
            corner(1, 1).into_any_element(),
            corner(1, -1).into_any_element(),
            corner(-1, 1).into_any_element(),
        ]
    }

    fn resize_press(&mut self, id: &str, sx: i8, sy: i8, button: MouseButton, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(start) = self.bounds.borrow().get(&format!("overlay:{id}")).copied() else { return };
        self.raise_window(id, cx);
        self.win_resize = Some(WinResize { id: id.to_owned(), sx, sy, button, down: at, start, moved: false });
        cx.notify();
    }

    /// Shift + right press: resizes by the corner of the quarter pressed, as Hyprland does.
    fn resize_by_quarter(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(b) = self.bounds.borrow().get(&format!("overlay:{id}")).copied() else { return };
        let (sx, sy) = (if at.x < b.center().x { -1 } else { 1 }, if at.y < b.center().y { -1 } else { 1 });
        self.resize_press(id, sx, sy, MouseButton::Right, at, cx);
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
        let (mut x, mut w) = span(r.sx, b.origin.x, b.size.width, d.x, min_w);
        let (mut y, mut h) = span(r.sy, b.origin.y, b.size.height, d.y, min_h);
        let (id, vertical, sx, sy) = (r.id.clone(), r.sy != 0, r.sx, r.sy);
        let area = self.overlays.iter().find(|o| o.id == id).and_then(|o| self.pane_area(o));
        if let Some(a) = area {
            let (right, bottom) = ((x + w).min(a.right()), (y + h).min(a.bottom()));
            (x, y) = (x.max(a.left()), y.max(a.top()));
            (w, h) = (right - x, bottom - y);
        }
        let zoom = settings::zoom();
        let mut unfitted = None;
        if let Some(o) = self.overlays.iter_mut().find(|o| o.id == id) {
            if o.fit && vertical {
                unfitted = Some(o.slot.clone());
            }
            match area {
                Some(a) => {
                    let before = Dock::of(b, a);
                    let right = if sx == 0 { before.right } else { sx < 0 };
                    let bottom = if sy == 0 { before.bottom } else { sy < 0 };
                    o.dock = Some(Dock::anchored(Bounds { origin: point(x, y), size: size(w, h) }, a, right, bottom));
                }
                None => o.place = Some(point(x, y)),
            }
            o.fit = o.fit && !vertical;
            o.size = size(f32::from(w) / zoom, if o.fit { o.size.height } else { f32::from(h) / zoom });
        }
        if let Some(root) = unfitted.and_then(|slot| self.copies.get(&slot)).and_then(|c| c.root.clone()) {
            root.update(cx, |r, cx| r.set_fit_content(false, cx));
        }
        cx.notify();
    }

    /// Where a pane window may be while its pane is shown: the pane below or
    /// above its header, less its margin (None: a free window, or its pane is
    /// hidden and so is the window).
    pub(super) fn pane_area(&self, o: &Overlay) -> Option<Bounds<Pixels>> {
        let pane = o.pane.as_ref()?;
        let b = self.shown_panes().contains(pane).then(|| self.bounds.borrow().get(pane).copied()).flatten()?;
        let header = self.pane_frame().and_then(|f| f.header.as_ref()).filter(|h| !h.hover());
        let row = header.map_or(px(0.), |h| settings::z(super::pane_size::header_height(h)).min(b.size.height / 2.));
        let top = if header.is_some_and(|h| !h.bottom()) { row } else { px(0.) };
        let b = Bounds { origin: point(b.origin.x, b.origin.y + top), size: size(b.size.width, b.size.height - row) };
        let m = settings::z(PANE_MARGIN).min(b.size.width / 4.).min(b.size.height / 4.);
        Some(Bounds { origin: point(b.origin.x + m, b.origin.y + m), size: size(b.size.width - m * 2., b.size.height - m * 2.) })
    }

    /// Whether a shown window overlay lies under this point, so a wheel tilt there is its own.
    pub(super) fn window_under(&self, at: Point<Pixels>) -> bool {
        let bounds = self.bounds.borrow();
        self.overlays.iter().filter(|o| o.kind == OverlayKind::Window && (o.pane.is_none() || self.pane_area(o).is_some()))
            .any(|o| bounds.get(&format!("overlay:{}", o.id)).is_some_and(|b| b.contains(&at)))
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
            match (o.pane.is_some(), o.place, o.dock) {
                (true, _, Some(d)) => remember_dock(k, d),
                (false, Some(p), _) => remember(k, p),
                _ => {}
            }
            remember_size(k, o.size);
        }
        true
    }

    pub(super) fn raise_window(&mut self, id: &str, cx: &mut Context<Self>) {
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
        let Some(i) = self.overlays.iter().position(|o| o.id == d.id) else { return true };
        let area = self.pane_area(&self.overlays[i]);
        let measured = self.bounds.borrow().get(&self.overlays[i].slot).map(|b| b.size).unwrap_or_default();
        let o = &mut self.overlays[i];
        match area {
            Some(a) => {
                let (x, y) = inside(at, measured, a);
                let dock = Dock::of(Bounds { origin: point(x, y), size: measured }, a);
                o.dock = Some(dock);
                if let Some(k) = &o.key {
                    remember_dock(k, dock);
                }
            }
            None if o.pane.is_none() => {
                o.place = Some(at);
                if let Some(k) = &o.key {
                    remember(k, at);
                }
            }
            None => {}
        }
        true
    }

    /// While a window is pressed: follows the pointer anywhere in the app.
    pub(super) fn window_tracker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.win_drag.is_none() && self.win_resize.is_none() {
            return None;
        }
        let this = cx.entity().downgrade();
        let layer = div().absolute().size_full();
        let layer = match &self.win_resize {
            Some(r) => layer.cursor(resize_cursor(r.sx, r.sy)),
            None => layer,
        };
        Some(
            layer.child(canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let t = this.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = t.update(cx, |this, cx| {
                                let resizing = this.win_resize.as_ref().map(|r| r.button);
                                this.window_move(ev.position, ev.pressed_button == Some(MouseButton::Left), cx);
                                this.resize_move(ev.position, resizing.is_some() && ev.pressed_button == resizing, cx);
                            });
                        }
                    });
                    let t = this.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                        let release = |this: &mut AppWindow, cx: &mut Context<AppWindow>| {
                            let drag = ev.button == MouseButton::Left && this.window_release(cx);
                            let resize = this.win_resize.as_ref().is_some_and(|r| r.button == ev.button) && this.resize_release(cx);
                            drag | resize
                        };
                        if phase == DispatchPhase::Capture && t.update(cx, release).unwrap_or(false) {
                            cx.stop_propagation();
                        }
                    });
                },
            )
            .absolute()
            .size_full())
            .into_any_element(),
        )
    }
}
