//! A pane's own sidebar: mod tabs on its left or right, inside its frame.
//! The pane draws in the width left over, so a terminal gets fewer columns
//! and its program reflows. The sidebar is part of the pane in the tab tree:
//! it persists, moves and swaps with it.

use super::*;
use crate::layout::{Edge, sidebar_owner, sidebar_slot};

/// The line between a pane and its sidebar, in px at zoom 1.
const DIVIDER: f32 = 1.0;
const HEADER: f32 = 30.0;
/// The least a pane keeps when its sidebar is dragged wider.
const PANE_MIN: f32 = 80.0;

/// The width a block's open sidebar takes from it, in px at zoom 1.
pub(super) fn taken(n: &Node) -> f32 {
    n.sidebar.as_deref().filter(|s| s.shown()).map_or(0.0, |s| s.px() + DIVIDER)
}

/// The mod copies of a block's sidebar: all its tabs, or only the one shown.
/// Each is told its `pane`, `terminal` and `sidebar` (the tab id) in props.
pub(super) fn sidebar_mods(n: &Node, all: bool) -> Vec<(String, Instance)> {
    let Some(s) = n.sidebar.as_deref() else { return Vec::new() };
    let tabs: Vec<&Node> = if all { s.tabs().iter().collect() } else { s.shown().then(|| s.stack.active_child()).flatten().into_iter().collect() };
    tabs.into_iter()
        .filter_map(|t| match t.content() {
            Some(Content::Mod { name, props }) => {
                let mut props = if props.is_object() { props.clone() } else { json!({}) };
                props["pane"] = json!(n.id);
                props["terminal"] = json!(n.terminal());
                props["sidebar"] = json!(t.id);
                Some((sidebar_slot(&n.id, &t.id), Instance { name: name.clone(), props }))
            }
            _ => None,
        })
        .collect()
}

/// What mods see of a pane's sidebar in `panes()`.
pub(super) fn sidebar_info(n: &Node) -> Value {
    let Some(s) = n.sidebar.as_deref() else { return Value::Null };
    let tabs: Vec<Value> = s
        .tabs()
        .iter()
        .map(|t| {
            let name = match t.content() {
                Some(Content::Mod { name, .. }) => Some(name),
                _ => None,
            };
            json!({ "id": t.id, "mod": name, "title": t.title })
        })
        .collect();
    json!({ "open": s.shown(), "side": s.side, "px": s.px(), "active": s.stack.active_child().map(|t| &t.id), "tabs": tabs })
}

impl AppWindow {
    /// The pane `pane.sidebar.*` acts on: `pane`, else `terminal`'s, else the
    /// one whose sidebar has the keyboard, else the focused pane.
    fn sidebar_pane(&self, a: &Value) -> Option<String> {
        Self::owned(a, "pane")
            .or_else(|| Self::arg(a, "terminal").and_then(|t| self.model.pane_of(t)))
            .or_else(|| self.focused_block.as_deref().and_then(sidebar_owner).map(str::to_owned))
            .or_else(|| self.model.focused_pane())
            .filter(|p| self.model.tab_of(p).is_some())
    }

    /// The tab a pane's sidebar shows, if it is open.
    fn sidebar_shown(&mut self, pane: &str) -> Option<String> {
        let s = self.find_node_mut(pane)?.sidebar.as_deref()?;
        s.shown().then(|| s.stack.active_child().map(|t| t.id.clone())).flatten()
    }

    /// `pane.sidebar.open`, `.close`, `.toggle` and `.focus`.
    pub(super) fn sidebar_command(&mut self, name: &str, a: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.sidebar_pane(a) else { return self.fail("no such pane") };
        let shown = self.sidebar_shown(&pane);
        let tab = Self::owned(a, "id").or_else(|| Self::owned(a, "mod"));
        let open = match name {
            "pane.sidebar.focus" => return self.focus_sidebar(&pane, shown, window, cx),
            "pane.sidebar.close" => false,
            "pane.sidebar.toggle" => shown.is_none() || tab.is_some_and(|t| shown.as_ref() != Some(&t)),
            _ => true,
        };
        if !open {
            let id = if name == "pane.sidebar.close" { Self::arg(a, "id") } else { None };
            let closed = self.find_node_mut(&pane).is_some_and(|n| n.close_sidebar(id));
            if !closed && let Some(id) = id {
                return self.fail(format!("no sidebar tab {id:?}"));
            }
            return self.sidebar_changed(&pane, None, window, cx);
        }
        let opened = match Self::arg(a, "mod") {
            Some(m) => self.open_sidebar_tab(&pane, m, a),
            None => {
                let id = Self::arg(a, "id");
                let sidebar = self.find_node_mut(&pane).and_then(|n| n.sidebar.as_mut());
                match sidebar.map(|s| s.show(id).then(|| s.stack.active_child().map(|t| t.id.clone()).unwrap_or_default())) {
                    Some(Some(tab)) => Ok(tab),
                    Some(None) if let Some(id) = id => Err(format!("no sidebar tab {id:?}")),
                    _ => Err("the pane has no sidebar tabs; pass a mod".into()),
                }
            }
        };
        match opened {
            Ok(tab) => {
                let focus = a.get("focus").and_then(Value::as_bool).unwrap_or(false);
                self.sidebar_changed(&pane, focus.then_some(tab), window, cx);
            }
            Err(e) => self.fail(e),
        }
    }

    /// Adds (or replaces) a mod tab in the pane's sidebar and shows it. A new
    /// sidebar starts as the mod.json `"sidebar"` hint says; given args win.
    fn open_sidebar_tab(&mut self, pane: &str, name: &str, a: &Value) -> Result<String, String> {
        let hint = self.folders.get(name).ok_or_else(|| format!("no mod folder {name:?}"))?.hint.sidebar.clone().unwrap_or_default();
        let side = match Self::arg(a, "side") {
            None => None,
            Some("left") => Some(Edge::Left),
            Some("right") => Some(Edge::Right),
            Some(s) => return Err(format!("side {s:?}: left or right")),
        };
        let id = Self::owned(a, "id").unwrap_or_else(|| name.to_owned());
        let mut tab = Node::block(&id, Content::Mod { name: name.to_owned(), props: a.get("props").cloned().unwrap_or(Value::Null) });
        tab.title = Some(Self::owned(a, "title").or(hint.title).unwrap_or_else(|| name.to_owned()));
        let number = |k: &str| a.get(k).and_then(Value::as_f64).map(|v| v as f32);
        let n = self.find_node_mut(pane).ok_or("no such pane")?;
        let new = n.sidebar.is_none();
        let s = n.open_sidebar(tab);
        let fresh = |given: Option<f32>, hinted: Option<f32>| given.or(hinted.filter(|_| new));
        if let Some(side) = side.or(hint.side.filter(|_| new)) {
            s.side = if side == Edge::Left { Edge::Left } else { Edge::Right };
        }
        if let Some(px) = fresh(number("px"), hint.px) {
            s.stack.size = Some(Span::Px { px });
        }
        if let Some(min) = fresh(number("min"), hint.min) {
            s.stack.min = Some(min);
        }
        if let Some(max) = fresh(number("max"), hint.max) {
            s.stack.max = Some(max);
        }
        Ok(id)
    }

    /// After a sidebar opened, closed or changed tab: the keyboard goes to
    /// `focus` (a tab id), or back to the pane if its sidebar had it and closed.
    fn sidebar_changed(&mut self, pane: &str, focus: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_menu = None;
        self.publish(cx);
        self.save();
        if let Some(tab) = focus {
            return self.focus_block(&sidebar_slot(pane, &tab), window, cx);
        }
        let had_keys = self.focused_block.as_deref().and_then(sidebar_owner) == Some(pane);
        if had_keys && !self.focused_block.as_deref().is_some_and(|b| self.mod_block_shown(b)) {
            self.back_to_pane(pane, window, cx);
        }
    }

    /// Keyboard into the pane's sidebar, or back to the pane if it is there.
    fn focus_sidebar(&mut self, pane: &str, shown: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused_block.as_deref().and_then(sidebar_owner) == Some(pane) {
            return self.back_to_pane(pane, window, cx);
        }
        match shown {
            Some(tab) => self.focus_block(&sidebar_slot(pane, &tab), window, cx),
            None => self.fail("the pane's sidebar is closed"),
        }
    }

    fn back_to_pane(&mut self, pane: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_block = None;
        self.model.focus_pane(pane);
        self.focused_pane = None;
        self.sync(window, cx);
    }

    /// A press in a sidebar tab: it holds the keyboard from now on, unless
    /// something inside (an input) took it.
    pub(super) fn enter_sidebar(&mut self, slot: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = sidebar_owner(slot).map(str::to_owned) else { return };
        if self.focused_block.as_deref() == Some(slot) {
            return;
        }
        let terminal_kept = self.pane_focus(&pane, cx).is_some_and(|f| f.is_focused(window));
        self.focused_block = Some(slot.to_owned());
        self.model.focus_pane(&pane);
        self.focused_pane = Some(pane);
        self.synced_model_pane = self.model.focused_pane();
        if terminal_kept || window.focused(cx).is_none() {
            self.focus.focus(window, cx);
            self.hand_keyboard();
        }
        self.publish(cx);
        self.save();
        cx.notify();
    }

    /// A sidebar tab on screen (its pane in the shown tab, the tab active).
    pub(super) fn sidebar_tab_shown(&self, slot: &str) -> bool {
        let Some(pane) = sidebar_owner(slot) else { return false };
        self.model
            .active_tab()
            .and_then(|t| t.shown().visible_blocks().into_iter().find(|n| n.id == pane))
            .is_some_and(|n| sidebar_mods(n, false).iter().any(|(s, _)| s == slot))
    }

    /// Dragging the line between a pane and its sidebar.
    pub(super) fn drag_sidebar(&mut self, pane: &str, pos: Point<Pixels>) {
        let Some((side, slot, min, max)) = self.find_node_mut(pane).and_then(|n| n.sidebar.as_deref()).and_then(|s| {
            let tab = s.stack.active_child()?;
            Some((s.side, sidebar_slot(pane, &tab.id), s.stack.min.unwrap_or(crate::layout::Sidebar::MIN), s.stack.max.unwrap_or(f32::INFINITY)))
        }) else {
            return;
        };
        let bounds = self.bounds.borrow();
        let (Some(b), Some(whole)) = (bounds.get(&slot).copied(), bounds.get(pane).copied()) else { return };
        drop(bounds);
        let zoom = settings::zoom();
        let x = f32::from(pos.x);
        let width = if side == Edge::Left { x - f32::from(b.left()) } else { f32::from(b.right()) - x } / zoom;
        let room = f32::from(whole.size.width) / zoom - DIVIDER - PANE_MIN;
        let px = width.min(room).min(max).max(min).round();
        if let Some(s) = self.find_node_mut(pane).and_then(|n| n.sidebar.as_mut()) {
            s.stack.size = Some(Span::Px { px });
        }
        self.hold_growth(pane);
    }

    /// The block's own element with its open sidebar beside it.
    pub(super) fn with_sidebar(&self, n: &Node, inner: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let Some(s) = n.sidebar.as_deref().filter(|s| s.shown()) else { return inner };
        let zoom = settings::zoom();
        let main = div().flex_1().min_w_0().h_full().flex().child(inner);
        let body = if self.sidebar_menu.as_deref() == Some(n.id.as_str()) { self.sidebar_picker(&n.id, cx) } else { self.sidebar_body(n, cx) };
        let side = div()
            .flex_none()
            .w(px(s.px() * zoom))
            .h_full()
            .flex()
            .flex_col()
            .bg(c("surface"))
            .child(div().flex_none().h(settings::z(HEADER)).w_full().flex().child(self.sidebar_header(n, cx)))
            .child(div().flex_1().min_h_0().flex().child(body));
        let line = self.sidebar_divider(&n.id, cx);
        let row = div().size_full().flex().flex_row();
        let row = if s.side == Edge::Left { row.child(side).child(line).child(main) } else { row.child(main).child(line).child(side) };
        row.into_any_element()
    }

    fn sidebar_body(&self, n: &Node, cx: &mut Context<Self>) -> AnyElement {
        let Some((slot, _)) = sidebar_mods(n, false).into_iter().next() else { return div().into_any_element() };
        let bounds_map = self.bounds.clone();
        let id = slot.clone();
        div()
            .relative()
            .size_full()
            .flex()
            .child(canvas(move |b, _, _| { bounds_map.borrow_mut().insert(id, b); }, |_, _, _, _| {}).absolute().size_full())
            .child(self.render_slot(&slot, cx))
            .into_any_element()
    }

    fn sidebar_header(&self, n: &Node, cx: &mut Context<Self>) -> AnyElement {
        let Some(s) = n.sidebar.as_deref() else { return div().into_any_element() };
        let active = s.stack.active_child().map(|t| t.id.clone());
        let tabs = s
            .tabs()
            .iter()
            .map(|t| StripTab {
                key: sidebar_slot(&n.id, &t.id),
                title: t.title.clone().unwrap_or_else(|| t.id.clone()),
                on: active.as_deref() == Some(t.id.as_str()),
                select: ("pane.sidebar.open", json!({ "pane": n.id, "id": t.id })),
                close: Some(("pane.sidebar.close", json!({ "pane": n.id, "id": t.id }))),
            })
            .collect();
        let pane = n.id.clone();
        let addable = self.folders.values().any(|f| f.hint.sidebar.is_some());
        let button = |id: String, label: &'static str| {
            div().id(SharedString::from(id)).w(settings::z(26.0)).h_full().flex().items_center().justify_center().text_color(c("muted")).hover(|d| d.text_color(c("foreground")).bg(c("raised"))).child(label)
        };
        self.tab_strip(tabs, cx)
            .child(div().flex_1())
            .when(addable, |d| {
                let pane = pane.clone();
                d.child(button(format!("sidebar-add-{pane}"), "+").on_click(cx.listener(move |this, _, _, cx| {
                    this.sidebar_menu = if this.sidebar_menu.as_deref() == Some(pane.as_str()) { None } else { Some(pane.clone()) };
                    cx.notify();
                })))
            })
            .child(button(format!("sidebar-close-{pane}"), "✕").on_click(cx.listener(move |this, _, w, cx| this.run("pane.sidebar.close", json!({ "pane": pane }), w, cx))))
            .into_any_element()
    }

    /// The + list: mods whose mod.json offers a `"sidebar"` tab.
    fn sidebar_picker(&self, pane: &str, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().size_full().flex().flex_col().py_1().text_sm();
        for (name, f) in &self.folders {
            let Some(h) = &f.hint.sidebar else { continue };
            let args = json!({ "pane": pane, "mod": name, "focus": true });
            list = list.child(
                div()
                    .id(SharedString::from(format!("sidebar-pick-{pane}-{name}")))
                    .px_3()
                    .py_1()
                    .hover(|d| d.bg(c("raised")))
                    .child(h.title.clone().unwrap_or_else(|| name.clone()))
                    .on_click(cx.listener(move |this, _, w, cx| this.run("pane.sidebar.open", args.clone(), w, cx))),
            );
        }
        list.into_any_element()
    }

    fn sidebar_divider(&self, pane: &str, cx: &mut Context<Self>) -> AnyElement {
        let width = DIVIDER * settings::zoom();
        let pane = pane.to_owned();
        div()
            .flex_none()
            .relative()
            .w(px(width))
            .h_full()
            .bg(c("border"))
            .child(
                div().absolute().top_0().bottom_0().left(px((width - HANDLE) / 2.0)).w(px(HANDLE)).cursor_col_resize().on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        if let Some(space) = this.model.space_of(&pane)
                            && this.require_document(space.machine.as_deref()).is_err()
                        {
                            return;
                        }
                        this.begin_growth_resize(&pane);
                        this.drag = Some(Drag { container: format!("{pane}#sidebar"), index: 0, axis: Axis::Row });
                        cx.stop_propagation();
                    }),
                ),
            )
            .into_any_element()
    }
}
