#[test]
fn initial_pane_size_matches_the_rendered_terminal() {
    in_own_process("initial_pane_size");
}

#[gpui::test]
fn initial_pane_size(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    use super::Axis::{Column, Row};
    if !running_alone("initial_pane_size") { return; }
    let plain = json!({"id": "context", "type": "block", "content": {"tab": "active"}});
    let framed = json!({"id": "context", "type": "block", "padding": [8, 10],
        "border": {"width": 1, "color": "border"},
        "content": {"tab": "active", "pane": {
            "gap": 6, "border": {"width": 2, "color": "border"},
            "header": {"height": 23, "content": {"mod": "size-probe"}}
        }}
    });
    let main: serde_json::Value = serde_json::from_str(include_str!("../../defaults/layouts/main.json")).unwrap();
    let (view, cx) = open_window(cx, "size-probe", "export default () => null;", plain.clone());
    for (label, layout, steps) in [
        ("lone", plain.clone(), vec![]),
        ("right", plain.clone(), vec![Row]),
        ("down", plain.clone(), vec![Column]),
        ("nested", plain.clone(), vec![Row, Column, Row]),
        ("constrained-max", plain.clone(), vec![Row]),
        ("constrained-min", plain.clone(), vec![Row]),
        ("constrained-mixed", plain.clone(), vec![Row, Row]),
        ("thick-border", json!({"id": "context", "type": "block", "padding": [8, 10],
            "border": {"width": 20, "color": "border"}, "content": {"tab": "active"}}), vec![Row]),
        ("headers-padding", framed, vec![Row, Column]),
        ("main-right", main.clone(), vec![Row]),
        ("main-down", main.clone(), vec![Column]),
        ("main-nested", main, vec![Row, Column, Row]),
    ] {
        let mut root = crate::model::pane("a", "probe-a");
        let mut target = "a".to_owned();
        for (i, &axis) in steps.iter().enumerate().take(steps.len().saturating_sub(1)) {
            let next = format!("p{i}");
            root.split(&target, axis, crate::model::pane(&next, &format!("probe-{next}")), format!("s{i}"));
            target = next;
        }
        view.update_in(cx, |this, window, cx| {
            this.folders.retain(|id, _| id == "size-probe");
            this.chrome = serde_json::from_value(layout).unwrap();
            this.bounds.borrow_mut().clear();
            this.model = crate::model::Model::default();
            let mut tab = super::Tab::new("tab", "", "a", "probe-a");
            tab.root = root.clone();
            tab.focus(&target);
            this.model.add_space("space", "test", "/", tab);
            this.sync(window, cx);
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if let Some(&axis) = steps.last() {
            root.split(&target, axis, crate::model::pane("new", "probe-new"), "last".into());
            target = "new".into();
        }
        match label {
            "constrained-max" => root.find_mut("a").unwrap().max = Some(100.0),
            "constrained-min" => root.find_mut("a").unwrap().min = Some(1000.0),
            "constrained-mixed" => {
                root.find_mut("a").unwrap().min = Some(1100.0);
                root.find_mut("p0").unwrap().max = Some(100.0);
            }
            _ => {}
        }
        let predicted = view.update_in(cx, |this, window, _| this.initial_size(&root, &target, window)).expect(label);
        view.update_in(cx, |this, window, cx| {
            let tab = &mut this.model.spaces[0].tabs[0];
            tab.root = root.clone();
            tab.focus(&target);
            this.sync(window, cx);
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let actual = view.update(cx, |this, cx| this.terms[&target].view.read(cx).dimensions());
        let delta = (predicted.0.abs_diff(actual.0), predicted.1.abs_diff(actual.1));
        eprintln!("{label}: initial {predicted:?}, rendered {actual:?}, delta {delta:?} cells");
        assert_eq!(predicted, actual, "{label}: initial size must match the rendered grid exactly");
    }
}

#[test]
fn hidden_tabs_follow_the_window() {
    in_own_process("hidden_tabs_follow");
}

#[gpui::test]
fn hidden_tabs_follow(cx: &mut gpui::TestAppContext) {
    use gpui::{px, size};
    if !running_alone("hidden_tabs_follow") { return; }
    let main: serde_json::Value = serde_json::from_str(include_str!("../../defaults/layouts/main.json")).unwrap();
    let (view, cx) = open_window(cx, "size-probe", "export default () => null;", main);
    // Tab "a" has two panes side by side; tab "b" is the one shown.
    let mut root = crate::model::pane("a", "probe-a");
    root.split("a", super::Axis::Row, crate::model::pane("a2", "probe-a2"), "s".into());
    view.update_in(cx, |this, window, cx| {
        this.folders.retain(|id, _| id == "size-probe");
        this.model = crate::model::Model::default();
        let mut tab = super::Tab::new("ta", "", "a", "probe-a");
        tab.root = root;
        tab.focus("a2");
        this.model.add_space("space", "test", "/", tab);
        this.model.add_tab("space", super::Tab::new("tb", "", "b", "probe-b"));
        this.sync(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let dims = |cx: &mut gpui::VisualTestContext| view.update(cx, |this, cx| ["a", "a2"].map(|p| this.terms[p].view.read(cx).dimensions()));
    let before = dims(cx);
    cx.simulate_resize(size(px(1400.0), px(900.0)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // The first tick after a change waits for the window to settle; the second sizes them.
    view.update_in(cx, |this, window, cx| this.fit_hidden_tabs(window, cx));
    assert_eq!(dims(cx), before, "a tick right after a change must leave hidden tabs alone");
    view.update_in(cx, |this, window, cx| this.fit_hidden_tabs(window, cx));
    let fitted = dims(cx);
    eprintln!("hidden tab: {before:?} before the window grew, {fitted:?} once it settled");
    assert_ne!(fitted, before, "a settled window must size the hidden tab's terminals");
    // Shown, the panes draw with the size they were given: nothing changes.
    view.update_in(cx, |this, window, cx| {
        this.model.select_tab("ta");
        this.sync(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(dims(cx), fitted, "hidden tabs must have been sized as their panes draw them");
}

#[test]
fn native_growth_fills_the_context_and_preserves_fixed_minimum_and_maximum_sizes() {
    use super::pane_size::shares;
    use crate::layout::{Axis, Node, Size};
    let mut root = Node::container("root", Axis::Column, (0..5).map(|i| crate::model::pane(&i.to_string(), &i.to_string())).collect());
    let grown = crate::focus_grow::derive(&root, "0", [2400.0, 1400.0], [10.0, 20.0]);
    let sizes = shares(&grown.children().iter().collect::<Vec<_>>(), 1376.0, 1.0);
    assert!((sizes.iter().sum::<f32>() + 24.0 - 1400.0).abs() < 0.01);
    assert!(sizes.windows(2).all(|p| p[0] > p[1]));
    root.children_mut().unwrap()[0].size = Some(Size::Px { px: 200.0 });
    root.children_mut().unwrap()[1].min = Some(350.0);
    root.children_mut().unwrap()[2].max = Some(100.0);
    let grown = crate::focus_grow::derive(&root, "3", [2400.0, 1400.0], [10.0, 20.0]);
    let sizes = shares(&grown.children().iter().collect::<Vec<_>>(), 1376.0, 1.0);
    assert_eq!(sizes[0], 200.0);
    assert!(sizes[1] >= 350.0);
    assert!(sizes[2] <= 100.0);
    assert!(sizes.iter().sum::<f32>() <= 1376.01);
    assert_eq!(grown.find("0").unwrap().size, root.find("0").unwrap().size);
}

#[test]
fn native_growth_preserves_contested_group_directionality_and_fills_every_branch() {
    use crate::layout::{Axis, Node, Size};
    fn weighted(mut node: Node, weight: f32) -> Node {
        node.size = Some(Size::Weight { weight });
        node
    }
    fn spans(root: &Node, width: f32, height: f32, result: &mut std::collections::HashMap<String, (f32, f32)>) {
        let Some(axis) = root.axis() else { result.insert(root.id.clone(), (width, height)); return };
        let children = root.children().iter().collect::<Vec<_>>();
        let available = if axis == Axis::Row { width } else { height };
        let gaps = 6.0 * children.len().saturating_sub(1) as f32;
        let sizes = super::pane_size::shares(&children, available - gaps, 1.0);
        assert!((sizes.iter().sum::<f32>() + gaps - available).abs() < 0.01);
        for (child, span) in children.into_iter().zip(sizes) {
            let (w, h) = if axis == Axis::Row { (span, height) } else { (width, span) };
            spans(child, w, h, result);
        }
    }
    let pane = |id, weight| weighted(crate::model::pane(id, id), weight);
    let top = weighted(Node::container("top", Axis::Row, vec![pane("B", 54.0), pane("D", 92.0)]), 23.0);
    let bottom = weighted(Node::container("bottom", Axis::Row, vec![pane("C", 76.0), pane("E", 70.0)]), 23.0);
    let right = weighted(Node::container("right", Axis::Column, vec![top, bottom]), 147.0);
    let root = Node::container("root", Axis::Row, vec![pane("A", 40.0), right]);
    for height in [940.0, 980.0] {
        for target in ["C", "E"] {
            let grown = crate::focus_grow::derive(&root, target, [1880.0, height], [10.0, 20.0]);
            let mut sizes = std::collections::HashMap::new();
            spans(&grown, 1880.0, height, &mut sizes);
            let other = if target == "C" { "E" } else { "C" };
            assert!(sizes["D"].0 > sizes[other].0, "{target} at {height}: {sizes:?}");
            assert_eq!(grown.panes().iter().map(|p| &p.id).collect::<Vec<_>>(), root.panes().iter().map(|p| &p.id).collect::<Vec<_>>());
        }
    }
}

#[test]
fn focus_growth_uses_render_focus_without_writing_shared_layouts() {
    in_own_process("focus_growth_render");
}

#[test]
fn focus_growth_force_false_preserves_manual_resize() {
    in_own_process("focus_growth_force_command");
}

#[test]
fn focus_growth_waits_for_the_focusing_click_to_end() {
    in_own_process("focus_growth_click");
}

#[gpui::test]
fn focus_growth_click(cx: &mut gpui::TestAppContext) {
    use gpui::MouseButton;
    use serde_json::json;
    if !running_alone("focus_growth_click") { return; }
    let layout = json!({"id":"context","type":"block","content":{"tab":"active","pane":{"gap":6}}});
    let (view, cx) = open_window(cx, "size-probe", "export default () => null;", layout);
    view.update_in(cx, |this, window, cx| {
        this.model = crate::model::Model::default();
        let mut tab = super::Tab::new("tab", "", "a", "probe-a");
        tab.root.split("a", super::Axis::Row, crate::model::pane("b", "probe-b"), "split1".into());
        tab.root.split("b", super::Axis::Row, crate::model::pane("c", "probe-c"), "split2".into());
        tab.focus("a");
        this.model.add_space("space", "test", "/", tab);
        this.person.focus_grow = true;
        this.sync(window, cx);
    });
    for _ in 0..2 { cx.update(|window, cx| window.draw(cx).clear(cx)); }
    let (pressed, grid) = view.update(cx, |this, cx| (this.bounds.borrow()["b"], this.terms["b"].view.read(cx).dimensions()));
    let at = pressed.center();
    cx.simulate_mouse_down(at, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |this, cx| {
        assert_eq!(this.model.active_tab().unwrap().focused_pane().as_deref(), Some("b"));
        assert_eq!(this.bounds.borrow()["b"], pressed);
        assert_eq!(this.terms["b"].view.read(cx).dimensions(), grid);
    });
    cx.simulate_mouse_move(at, Some(MouseButton::Left), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |this, _| assert_eq!(this.bounds.borrow()["b"], pressed));
    cx.simulate_mouse_up(at, MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update_in(cx, |this, window, cx| {
        let bounds = this.bounds.borrow();
        assert!(bounds["b"].size.width > pressed.size.width);
        assert!(["a", "c"].into_iter().all(|id| bounds["b"].size.width > bounds[id].size.width));
        let predicted = this.shown_size(this.model.active_tab().unwrap(), "b", window).unwrap();
        assert_eq!(this.terms["b"].view.read(cx).dimensions(), predicted);
    });
    let pressed = view.update(cx, |this, _| this.bounds.borrow()["c"]);
    cx.simulate_mouse_down(pressed.center(), MouseButton::Left, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |this, _| assert_eq!(this.bounds.borrow()["c"], pressed));
    cx.simulate_mouse_move(pressed.center(), None, Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |this, _| assert!(this.bounds.borrow()["c"].size.width > pressed.size.width));
}

#[gpui::test]
fn focus_growth_force_command(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    if !running_alone("focus_growth_force_command") { return; }
    let layout = json!({"id":"context","type":"block","content":{"tab":"active","pane":{"gap":6}}});
    let (view, cx) = open_window(cx, "size-probe", "export default () => null;", layout);
    view.update_in(cx, |this, window, cx| {
        this.model = crate::model::Model::default();
        let mut tab = super::Tab::new("tab", "", "a", "probe-a");
        tab.root.split("a", super::Axis::Row, crate::model::pane("b", "probe-b"), "split1".into());
        tab.root.split("b", super::Axis::Row, crate::model::pane("c", "probe-c"), "split2".into());
        tab.focus("a");
        this.model.add_space("space", "test", "/", tab);
        this.person.focus_grow = true;
        this.sync(window, cx);
    });
    for _ in 0..2 { cx.update(|window, cx| window.draw(cx).clear(cx)); }
    view.update_in(cx, |this, window, cx| {
        this.documents.entry(String::new()).or_default().available = true;
        for (args, resumes) in [
            (json!({"force":false}), false),
            (json!({"force":true}), true),
            (json!({"enabled":true,"force":false}), true),
            (json!({"suspended":false,"force":false}), true),
            (json!({"enabled":false,"force":true}), false),
            (json!({"suspended":true,"force":true}), false),
            (json!({}), false),
        ] {
            assert!(this.run_as("tab.focus_grow", json!({"enabled":true,"suspended":false}), false, window, cx).get("error").is_none());
            this.grown_tab(this.model.active_tab().unwrap(), window).unwrap();
            assert!(this.run_as("layout.resize", json!({"id":"b","weight":1.0}), false, window, cx).get("error").is_none());
            let held = this.grown_tab(this.model.active_tab().unwrap(), window).unwrap();
            let document = crate::mirror::Document::capture(&this.model, None);
            let solves = this.growth.borrow().solves;
            assert!(this.run_as("tab.focus_grow", args.clone(), false, window, cx).get("error").is_none());
            let shown = this.grown_tab(this.model.active_tab().unwrap(), window).unwrap();
            assert_eq!(this.growth.borrow().solves, solves + u64::from(resumes), "{args}");
            if !resumes { assert!(std::rc::Rc::ptr_eq(&held, &shown), "{args}"); }
            assert_eq!(crate::mirror::Document::capture(&this.model, None), document, "{args}");
        }
        let solves = this.growth.borrow().solves;
        assert!(this.run_as("tab.focus_grow", json!({}), false, window, cx).get("error").is_none());
        this.grown_tab(this.model.active_tab().unwrap(), window).unwrap();
        assert_eq!(this.growth.borrow().solves, solves + 1);
    });
}

#[gpui::test]
fn focus_growth_render(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    if !running_alone("focus_growth_render") { return; }
    let layout = json!({"id":"chrome","type":"row","children":[
        {"id":"sidebar","type":"block","size":{"px":100},"content":{"mod":"size-probe"}},
        {"id":"context","type":"block","content":{"tab":"active","pane":{"gap":6}}}
    ]});
    let (view, cx) = open_window(cx, "size-probe", "export default () => null;", layout);
    view.update_in(cx, |this, window, cx| {
        this.model = crate::model::Model::default();
        let mut tab = super::Tab::new("tab", "", "a", "probe-a");
        tab.root.split("a", super::Axis::Row, crate::model::pane("b", "probe-b"), "split1".into());
        tab.root.split("b", super::Axis::Row, crate::model::pane("c", "probe-c"), "split2".into());
        tab.focus("a");
        this.model.add_space("space", "test", "/", tab);
        this.sync(window, cx);
    });
    for _ in 0..2 { cx.update(|window, cx| window.draw(cx).clear(cx)); }
    let before = view.update(cx, |this, _| crate::mirror::Document::capture(&this.model, None));
    let sidebar = view.update(cx, |this, _| this.bounds.borrow()["sidebar"]);
    for target in ["c", "b", "a"] {
        view.update_in(cx, |this, window, cx| { this.terms[target].focus.focus(window, cx); cx.notify(); });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.update_in(cx, |this, window, cx| {
            assert_eq!(this.model.active_tab().unwrap().focused_pane().as_deref(), Some(target));
            let predicted = this.shown_size(this.model.active_tab().unwrap(), target, window).unwrap();
            assert_eq!(predicted, this.terms[target].view.read(cx).dimensions());
            assert_eq!(crate::mirror::Document::capture(&this.model, None), before);
            assert_eq!(this.bounds.borrow()["sidebar"], sidebar);
            let bounds = this.bounds.borrow();
            let selected = bounds[target].size.width;
            assert!(["a", "b", "c"].into_iter().filter(|id| *id != target).all(|id| selected > bounds[id].size.width));
        });
    }
    view.update_in(cx, |this, window, cx| {
        let sizes = this.growth.borrow().displayed("tab").unwrap();
        this.run("tab.focus_grow", json!({"suspended":true}), window, cx);
        this.run("tab.focus_grow", json!({"force":true}), window, cx);
        this.grown_tab(this.model.active_tab().unwrap(), window);
        assert_eq!(this.growth.borrow().displayed("tab").unwrap(), sizes);
        this.run("tab.focus_grow", json!({"suspended":false}), window, cx);
        this.begin_growth_resize("a");
        this.model.tab_mut("tab").unwrap().root.find_mut("a").unwrap().size = Some(super::Span::Px { px: 200.0 });
        this.hold_growth("a");
        this.grown_tab(this.model.active_tab().unwrap(), window);
        assert_eq!(this.growth.borrow().displayed("tab").unwrap().find("a").unwrap().size, Some(super::Span::Px { px: 200.0 }));
        this.drag = Some(super::Drag { container: "tab".into(), index: 0, axis: super::Axis::Row });
        let solves = this.growth.borrow().solves;
        this.run("tab.focus_grow", json!({"force":true}), window, cx);
        this.grown_tab(this.model.active_tab().unwrap(), window);
        assert_eq!(this.growth.borrow().solves, solves);
        this.drag = None;
        this.hold_growth("a");
    });
}
