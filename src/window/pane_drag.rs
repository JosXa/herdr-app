//! App-owned pane grips and drag gestures, independent of header mods.
use super::*;
use crate::layout::{Axis, Edge};

pub(super) struct PaneDrag {
    whole_tab: bool,
    pane: String,
    tab: String,
    down: Point<Pixels>,
    at: Point<Pixels>,
    moving: bool,
    target: Option<(String, Option<Edge>)>,
    mod_target: Option<ModTarget>,
}

/// A space, tab, pane or `mod` target a mod reported under the pointer; `vertical` targets sit in a top-to-bottom list.
/// `owner` is the reporting mod, which a `mod` drop reaches through its `nextCommand()`.
struct ModTarget {
    kind: String,
    id: String,
    bounds: Bounds<Pixels>,
    vertical: bool,
    owner: String,
}

/// What dropping on a mod target runs. `after` picks the target side its line marks;
/// `first` moves the tab a pane drop creates to the top of its space.
struct ModDrop {
    command: &'static str,
    args: Value,
    after: bool,
    first: bool,
}

/// The outer quarter of a pane chooses its nearest edge; the middle swaps.
fn drop_zone(bounds: Bounds<Pixels>, at: Point<Pixels>) -> Option<Option<Edge>> {
    if !bounds.contains(&at) || bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
        return None;
    }
    let x = f32::from(at.x - bounds.origin.x) / f32::from(bounds.size.width);
    let y = f32::from(at.y - bounds.origin.y) / f32::from(bounds.size.height);
    let (distance, edge) = [(x, Edge::Left), (1. - x, Edge::Right), (y, Edge::Top), (1. - y, Edge::Bottom)]
        .into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
    Some((distance < 0.25).then_some(edge))
}

/// Where a tab dropped on `target`'s left or right half lands (1-based), or None when it stays put.
fn tab_drop_index(tabs: &[&str], source: &str, target: &str, after: bool) -> Option<usize> {
    if source == target { return None; }
    let target_at = tabs.iter().position(|t| *t == target)?;
    let source_at = tabs.iter().position(|t| *t == source);
    let landing = target_at - usize::from(source_at.is_some_and(|s| s < target_at)) + usize::from(after);
    (source_at != Some(landing)).then_some(landing + 1)
}

fn drops_after(bounds: Bounds<Pixels>, at: Point<Pixels>, vertical: bool) -> bool {
    if vertical { at.y >= bounds.origin.y + bounds.size.height / 2. } else { at.x >= bounds.origin.x + bounds.size.width / 2. }
}

/// The edge that puts a pane before or after `target` in the reading order of its split.
fn sibling_edge(parent: Option<Axis>, after: bool) -> Edge {
    match (parent, after) {
        (Some(Axis::Column), false) => Edge::Top,
        (Some(Axis::Column), true) => Edge::Bottom,
        (_, false) => Edge::Left,
        (_, true) => Edge::Right,
    }
}

fn edge_name(edge: Edge) -> &'static str {
    match edge { Edge::Left => "left", Edge::Right => "right", Edge::Top => "top", Edge::Bottom => "bottom" }
}

/// An accent line on the drop side of a target: across a vertical list's row, or upright between horizontal tabs.
fn drop_line(b: Bounds<Pixels>, after: bool, vertical: bool) -> AnyElement {
    let thickness = settings::z(3.);
    let line = div().absolute().rounded_full().bg(c("accent"));
    if vertical {
        let inset = (b.size.width * 0.15).min(settings::z(6.));
        let edge = if after { b.origin.y + b.size.height } else { b.origin.y };
        line.left(b.origin.x + inset).top(edge - thickness / 2.).w(b.size.width - inset * 2.).h(thickness).into_any_element()
    } else {
        let inset = b.size.height * 0.15;
        let edge = if after { b.origin.x + b.size.width } else { b.origin.x };
        line.left(edge - thickness / 2.).top(b.origin.y + inset).w(thickness).h(b.size.height - inset * 2.).into_any_element()
    }
}

/// The command a `mod` drop sends its owner: `drop` and JSON naming the target and the dragged tab or pane.
fn mod_drop_command(d: &PaneDrag, target: &str) -> String {
    let source = if d.whole_tab { "tab" } else { "pane" };
    format!("drop {}", json!({"target": target, source: d.pane}))
}

fn drop_box(b: Bounds<Pixels>) -> AnyElement {
    div().absolute().left(b.origin.x).top(b.origin.y).w(b.size.width).h(b.size.height)
        .bg(c("accent").opacity(0.18)).border_2().border_color(c("accent")).into_any_element()
}

/// The top row of a pane without a top header.
const STRIP: f32 = 18.;

fn top_strip(mut bounds: Bounds<Pixels>, row: f32, at: Point<Pixels>) -> bool {
    bounds.size.height = bounds.size.height.min(settings::z(row));
    bounds.contains(&at)
}

fn inherited_movable(root: &Node, id: &str) -> Option<bool> {
    root.path_to(id)?.iter().rev().find_map(|id| root.find(id)?.movable)
}

impl AppWindow {
    pub(super) fn begin_mod_drag(&mut self, kind: &str, id: &str, event: &Value, cx: &mut Context<Self>) {
        let Some(at) = event_point(event) else { return };
        if event["button"].as_str() != Some("left") { return; }
        if kind == "pane" {
            self.pane_press(id, at, cx);
        } else if kind == "tab" && self.model.tab(id).is_some()
            && self.require_document(self.model.space_of(id).and_then(|s| s.machine.as_deref())).is_ok() {
            self.pane_drag = Some(PaneDrag { whole_tab: true, pane: id.into(), tab: id.into(), down: at, at, moving: false, target: None, mod_target: None });
            cx.notify();
        }
    }

    pub(super) fn mod_drag_target(&mut self, kind: &str, id: &str, event: &Value, vertical: bool, owner: &str, cx: &mut Context<Self>) {
        let Some(drag) = self.pane_drag.as_ref() else { return };
        let source = self.model.space_of(&drag.pane);
        let target = if kind == "space" { self.model.space(id) } else if matches!(kind, "tab" | "pane") { self.model.space_of(id) } else { None };
        if kind != "mod" && source.zip(target).is_none_or(|(a, b)| a.machine != b.machine) { return; }
        if kind == "pane" && event_point(event).and_then(|at| self.pane_target(at)).is_some_and(|(pane, _)| pane == id) { return; }
        let Some(bounds) = event_bounds(event) else { return };
        self.pane_drag.as_mut().unwrap().mod_target = Some(ModTarget { kind: kind.into(), id: id.into(), bounds, vertical, owner: owner.into() });
        cx.notify();
    }

    pub(super) fn pane_movable(&self, pane: &str) -> bool {
        let Some(tab) = self.model.tab_of(pane) else { return false };
        if !tab.root.find(pane).is_some_and(Node::is_pane) || self.require_document(self.model.space_of(pane).and_then(|s| s.machine.as_deref())).is_err() {
            return false;
        }
        let owner = self.chrome.visible_blocks().into_iter().find(|n| matches!(n.content(), Some(Content::Context { .. })));
        inherited_movable(&tab.root, pane)
            .or_else(|| owner.and_then(|n| inherited_movable(&self.chrome, &n.id)))
            .unwrap_or(true)
    }

    pub(super) fn pane_grip(&self, pane: &str, header: Option<f32>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let row = header.unwrap_or(STRIP).max(STRIP);
        self.pane_grip_rows.borrow_mut().insert(pane.to_owned(), row);
        if !self.pane_movable(pane) { return None; }
        let dragging = self.pane_drag.as_ref().is_some_and(|d| d.pane == pane);
        if self.pane_grip_hover.as_deref() != Some(pane) && !dragging { return None; }
        let pane = pane.to_owned();
        Some(div().id(SharedString::from(format!("grip-{pane}")))
            .absolute().top(settings::z((row - 18.) / 2.)).left(relative(0.5)).ml(-settings::z(14.))
            .w(settings::z(28.)).h(settings::z(18.)).rounded(settings::z(4.))
            .bg(c("background").opacity(0.85))
            .hover(|s| s.bg(c("hover")))
            .flex().flex_col().items_center().justify_center().gap(settings::z(3.)).occlude()
            .group("pane-grip")
            .cursor(if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                this.pane_press(&pane, ev.position, cx);
                cx.stop_propagation();
            }))
            .children((0..2).map(|_| div().flex().gap(settings::z(3.)).children((0..3).map(|_| div().size(settings::z(2.)).rounded_full()
                .bg(if dragging { c("foreground") } else { c("dim") })
                .group_hover("pane-grip", |s| s.bg(c("foreground")))))))
            .into_any_element())
    }

    fn pane_hover(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let hovered = self.model.active_tab().and_then(|tab| {
            let bounds = self.bounds.borrow();
            let rows = self.pane_grip_rows.borrow();
            tab.shown().visible_blocks().into_iter().find_map(|n| {
                let b = bounds.get(&n.id)?;
                let row = rows.get(&n.id).copied().unwrap_or(STRIP);
                (self.pane_movable(&n.id) && top_strip(*b, row, at)).then(|| n.id.clone())
            })
        });
        if hovered != self.pane_grip_hover {
            self.pane_grip_hover = hovered;
            cx.notify();
        }
    }

    pub(super) fn pane_press(&mut self, pane: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.pane_movable(pane) {
            self.pane_drag = Some(PaneDrag {
                whole_tab: false, mod_target: None,
                pane: pane.to_owned(), tab: self.model.tab_of(pane).unwrap().id.clone(),
                down: at, at, moving: false, target: None,
            });
            cx.notify();
        }
    }

    fn pane_target(&self, at: Point<Pixels>) -> Option<(String, Option<Edge>)> {
        let d = self.pane_drag.as_ref()?;
        if d.whole_tab { return None; }
        let tab = self.model.active_tab().filter(|t| t.id == d.tab && t.zoomed.is_none())?;
        let bounds = self.bounds.borrow();
        tab.shown().visible_blocks().into_iter().filter(|n| n.is_pane() && n.id != d.pane)
            .find_map(|n| Some((n.id.clone(), drop_zone(*bounds.get(&n.id)?, at)?)))
    }

    fn pane_move(&mut self, at: Point<Pixels>, pressed: bool, cx: &mut Context<Self>) {
        let Some(d) = self.pane_drag.as_mut() else { return };
        if !pressed {
            self.cancel_pane_drag(cx);
            return;
        }
        if !d.moving && (at.x - d.down.x).abs().max((at.y - d.down.y).abs()) < settings::z(4.) {
            return;
        }
        d.moving = true;
        d.at = at;
        if d.mod_target.as_ref().is_some_and(|t| !t.bounds.contains(&at)) { d.mod_target = None; }
        let target = self.pane_target(at);
        self.pane_drag.as_mut().unwrap().target = target;
        cx.notify();
    }

    pub(super) fn cancel_pane_drag(&mut self, cx: &mut Context<Self>) {
        if self.pane_drag.take().is_some() {
            cx.notify();
        }
    }

    fn pane_release(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let target = self.pane_target(at);
        let Some(d) = self.pane_drag.take() else { return false };
        if !d.moving {
            cx.notify();
            return false;
        }
        if let Some(target) = d.mod_target.as_ref().filter(|t| t.bounds.contains(&at)) {
            if target.kind == "mod" {
                crate::mods::send_command(&target.owner, &mod_drop_command(&d, &target.id));
            } else if let Some(drop) = self.mod_drop(&d, target, at) {
                self.run(drop.command, drop.args, window, cx);
                if drop.first && let Some(tab) = self.model.tab_of(&d.pane).map(|t| t.id.clone()) {
                    self.run("tab.move", json!({"id":tab,"index":1}), window, cx);
                }
            }
        } else if !d.whole_tab && self.pane_movable(&d.pane) && let Some((target, edge)) = target {
            let edge = edge.map(edge_name);
            self.run("pane.move", json!({"pane":d.pane,"target":target,"edge":edge}), window, cx);
        }
        cx.defer_in(window, |this, window, cx| this.restore_focus(window, cx));
        cx.notify();
        true
    }

    fn tab_drop(&self, source: &str, target: &str, after: bool) -> Option<(String, usize)> {
        let space = self.model.space_of(target)?;
        let tabs: Vec<&str> = space.tabs.iter().map(|t| t.id.as_str()).collect();
        Some((space.id.clone(), tab_drop_index(&tabs, source, target, after)?))
    }

    fn mod_drop(&self, d: &PaneDrag, target: &ModTarget, at: Point<Pixels>) -> Option<ModDrop> {
        let ModTarget { kind, id, bounds, vertical, .. } = target;
        let after = drops_after(*bounds, at, *vertical);
        let drop = |command, args, after| Some(ModDrop { command, args, after, first: false });
        match (d.whole_tab, kind.as_str()) {
            (true, "tab") => {
                let (space, index) = self.tab_drop(&d.pane, id, after)?;
                drop("tab.move", json!({"id":d.pane,"space":space,"index":index}), after)
            }
            (true, "space") if *vertical => {
                let space = self.model.space(id)?;
                if space.tabs.first().is_some_and(|t| t.id == d.pane) { return None; }
                drop("tab.move", json!({"id":d.pane,"space":id,"index":1}), true)
            }
            (true, "space") => drop("tab.move", json!({"id":d.pane,"space":self.model.space(id)?.id}), true),
            (true, _) => None,
            (false, "pane") if *vertical => {
                if *id == d.pane { return None; }
                let parent = self.model.tab_of(id)?.root.parent_of(id).and_then(Node::axis);
                drop("pane.move", json!({"pane":d.pane,"target":id,"edge":edge_name(sibling_edge(parent, after))}), after)
            }
            (false, "pane") => drop("pane.move", json!({"pane":d.pane,"target":id}), true),
            (false, "space") => Some(ModDrop { command: "pane.move", args: json!({"pane":d.pane,"space":id}), after: true, first: *vertical }),
            (false, _) => drop("pane.move", json!({"pane":d.pane,kind.as_str():id}), true),
        }
    }

    pub(super) fn pane_drag_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let mod_preview = self.pane_drag.as_ref().filter(|d| d.moving).and_then(|d| {
            let target = d.mod_target.as_ref()?;
            if target.kind == "mod" { return Some(drop_box(target.bounds)); }
            let drop = self.mod_drop(d, target, d.at)?;
            let line = target.vertical || (d.whole_tab && target.kind == "tab");
            Some(if line { drop_line(target.bounds, drop.after, target.vertical) } else { drop_box(target.bounds) })
        });
        let preview = mod_preview.or_else(|| self.pane_drag.as_ref().and_then(|d| d.target.as_ref()).and_then(|(id, edge)| {
            let mut b = *self.bounds.borrow().get(id)?;
            match edge {
                Some(Edge::Left) => b.size.width /= 2.,
                Some(Edge::Right) => { b.size.width /= 2.; b.origin.x += b.size.width; }
                Some(Edge::Top) => b.size.height /= 2.,
                Some(Edge::Bottom) => { b.size.height /= 2.; b.origin.y += b.size.height; }
                None => {}
            }
            Some(drop_box(b))
        }));
        let this = cx.entity().downgrade();
        div().id("pane-drag-layer").absolute().inset_0()
            .when(self.pane_drag.as_ref().is_some_and(|d| d.moving), |d| d.cursor(CursorStyle::ClosedHand))
            .children(preview).child(canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let t = this.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = t.update(cx, |this, cx| {
                            this.pane_hover(ev.position, cx);
                            this.pane_move(ev.position, ev.pressed_button == Some(MouseButton::Left), cx);
                        });
                    }
                });
                let t = this.clone();
                window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && ev.button == MouseButton::Left
                        && t.update(cx, |this, cx| this.pane_release(ev.position, window, cx)).unwrap_or(false)
                    { cx.stop_propagation(); }
                });
            },
        ).absolute().size_full()).into_any_element()
    }
}

fn event_point(event: &Value) -> Option<Point<Pixels>> {
    Some(point(px(event["position"]["x"].as_f64()? as f32), px(event["position"]["y"].as_f64()? as f32)))
}

fn event_bounds(event: &Value) -> Option<Bounds<Pixels>> {
    let b = &event["bounds"];
    Some(Bounds::new(point(px(b["x"].as_f64()? as f32), px(b["y"].as_f64()? as f32)), size(px(b["width"].as_f64()? as f32), px(b["height"].as_f64()? as f32))))
}

#[cfg(test)]
mod tests {
    use super::{STRIP, drop_zone, drops_after, inherited_movable, sibling_edge, tab_drop_index, top_strip};
    use crate::layout::{Axis, Content, Edge, Node};
    use gpui::{Bounds, point, px, size};
    #[test]
    fn zones_are_relative_to_the_pane_and_outside_cancels() {
        let b = Bounds::new(point(px(100.), px(200.)), size(px(800.), px(400.)));
        for (x, y, expected) in [(500., 400., Some(None)), (110., 400., Some(Some(Edge::Left))),
            (890., 400., Some(Some(Edge::Right))), (500., 210., Some(Some(Edge::Top))),
            (500., 590., Some(Some(Edge::Bottom))), (99., 400., None)] {
            assert_eq!(drop_zone(b, point(px(x), px(y))), expected);
        }
    }
    #[test]
    fn tab_drops_land_on_the_hovered_side_and_skip_no_ops() {
        let tabs = ["a", "b", "c", "d"];
        for (source, target, after, expected) in [
            ("a", "c", false, Some(2)), ("a", "c", true, Some(3)), ("d", "b", false, Some(2)),
            ("d", "b", true, Some(3)), ("a", "d", true, Some(4)), ("d", "a", false, Some(1)),
            ("b", "c", false, None), ("c", "b", true, None), ("b", "b", true, None),
            ("x", "b", false, Some(2)), ("x", "d", true, Some(5)),
        ] {
            assert_eq!(tab_drop_index(&tabs, source, target, after), expected, "{source} on {target} after={after}");
        }
    }
    #[test]
    fn vertical_lists_split_rows_by_height() {
        let b = Bounds::new(point(px(0.), px(100.)), size(px(300.), px(26.)));
        assert!(!drops_after(b, point(px(290.), px(112.)), true));
        assert!(drops_after(b, point(px(10.), px(114.)), true));
        assert!(drops_after(b, point(px(290.), px(112.)), false));
    }
    #[test]
    fn pane_lines_follow_the_split_reading_order() {
        for (parent, after, expected) in [(Some(Axis::Row), false, Edge::Left), (Some(Axis::Row), true, Edge::Right),
            (Some(Axis::Column), false, Edge::Top), (Some(Axis::Column), true, Edge::Bottom), (None, true, Edge::Right)] {
            assert_eq!(sibling_edge(parent, after), expected);
        }
    }
    #[test]
    fn hover_strip_is_only_the_panes_top_edge() {
        let b = Bounds::new(point(px(100.), px(200.)), size(px(800.), px(400.)));
        assert!(top_strip(b, STRIP, point(px(110.), px(201.))));
        assert!(!top_strip(b, STRIP, point(px(110.), px(250.))));
        assert!(!top_strip(b, STRIP, point(px(99.), px(201.))));
        assert!(!top_strip(b, STRIP, point(px(500.), px(199.))));
        // A 40px top header is the whole row.
        assert!(top_strip(b, 40., point(px(110.), px(235.))));
        assert!(!top_strip(b, 40., point(px(110.), px(245.))));
    }
    #[test]
    fn movable_uses_the_nearest_explicit_ancestor() {
        let mut root = Node::container("root", Axis::Row, vec![Node::container("inner", Axis::Column,
            vec![Node::block("pane", Content::Terminal { terminal: "t1".into() })])]);
        assert_eq!(inherited_movable(&root, "pane"), None);
        root.movable = Some(false);
        assert_eq!(inherited_movable(&root, "pane"), Some(false));
        root.find_mut("inner").unwrap().movable = Some(true);
        assert_eq!(inherited_movable(&root, "pane"), Some(true));
    }
}
