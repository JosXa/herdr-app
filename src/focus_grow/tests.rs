use super::*;
use super::solver::{Kind as SolverKind, Node as SolverNode, Solver};

fn parse(solver: &mut Solver, text: &[u8], at: &mut usize, parent: Option<usize>, index: usize, ids: &mut Vec<i64>, target: i64) -> usize {
    fn number(text: &[u8], at: &mut usize) -> i64 {
        let start = *at;
        while text.get(*at).is_some_and(u8::is_ascii_digit) { *at += 1; }
        std::str::from_utf8(&text[start..*at]).unwrap().parse().unwrap()
    }
    let width = number(text, at); assert_eq!(text[*at], b'x'); *at += 1;
    let height = number(text, at); assert_eq!(text[*at], b','); *at += 1;
    let left = number(text, at); assert_eq!(text[*at], b','); *at += 1;
    let top = number(text, at);
    let id = solver.nodes.len();
    solver.nodes.push(SolverNode::new([width, height], [left, top], SolverKind::Leaf, parent, index));
    ids.push(-1);
    match text[*at] {
        b',' => {
            *at += 1;
            ids[id] = number(text, at);
            if ids[id] == target { solver.focus = id; }
            solver.leaves.push(id);
        }
        open @ (b'{' | b'[') => {
            *at += 1;
            solver.nodes[id].kind = if open == b'{' { SolverKind::X } else { SolverKind::Y };
            loop {
                let child = parse(solver, text, at, Some(id), solver.nodes[id].children.len(), ids, target);
                solver.nodes[id].children.push(child);
                if text.get(*at) == Some(&b',') { *at += 1; continue; }
                assert_eq!(text[*at], if open == b'{' { b'}' } else { b']' });
                *at += 1;
                break;
            }
        }
        _ => panic!("invalid fixture"),
    }
    id
}

fn emit(solver: &Solver, ids: &[i64], id: usize) -> String {
    let n = &solver.nodes[id];
    let prefix = format!("{}x{},{},{}", n.size[0], n.size[1], n.pos[0], n.pos[1]);
    if n.kind == SolverKind::Leaf { format!("{prefix},{}", ids[id]) }
    else { format!("{prefix}{}{}{}", if n.kind == SolverKind::X { '{' } else { '[' }, n.children.iter().map(|&c| emit(solver, ids, c)).collect::<Vec<_>>().join(","), if n.kind == SolverKind::X { '}' } else { ']' }) }
}

fn fit_width(solver: &mut Solver, id: usize, width: i64) {
    solver.nodes[id].size[0] = width.max(1);
    let children = solver.nodes[id].children.clone();
    if children.is_empty() { return; }
    if solver.nodes[id].kind == SolverKind::Y {
        for child in children { fit_width(solver, child, width.max(1)); }
    } else {
        let available = width.max(1) - children.len() as i64 + 1;
        if available <= 0 { return; }
        let weights: Vec<_> = children.iter().map(|&c| solver.nodes[c].size[0].max(1)).collect();
        let sizes = solver.allocate(0, available, &weights);
        for (&child, size) in children.iter().zip(sizes) { fit_width(solver, child, size); }
    }
}

fn rail(solver: &mut Solver, leaf: usize, mut width: i64) {
    let mut branch = leaf;
    while let Some(parent) = solver.nodes[branch].parent {
        if solver.nodes[parent].kind != SolverKind::X { branch = parent; continue; }
        let children = solver.nodes[parent].children.clone();
        if children.len() <= 1 { return; }
        let receiver = children.iter().copied().find(|&c| c != branch && solver.nodes[c].contains).unwrap_or_else(|| {
            let i = solver.nodes[branch].index;
            children[if i + 1 < children.len() { i + 1 } else { i - 1 }]
        });
        let mut delta = solver.nodes[branch].size[0] - width;
        if delta == 0 { return; }
        if delta < 0 {
            let shrink = solver.nodes[receiver].size[0] - 8;
            if shrink <= 0 { return; }
            if -delta > shrink { delta = -shrink; width = solver.nodes[branch].size[0] - delta; }
        }
        let receiver_width = solver.nodes[receiver].size[0] + delta;
        fit_width(solver, branch, width);
        fit_width(solver, receiver, receiver_width);
        return;
    }
}

fn corpus() -> Vec<serde_json::Value> {
    include_str!("../../tests/focus-grow-layout.jsonl").lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn exact_tmux_arithmetic_corpus() {
    let cases = corpus();
    assert_eq!(cases.len(), 298);
    for (i, case) in cases.iter().enumerate() {
        let args = case["args"].as_array().unwrap();
        let layout = args[0].as_str().unwrap();
        let mut solver = Solver::new([args[2].as_str().unwrap().parse().unwrap(), args[3].as_str().unwrap().parse().unwrap()]);
        let mut ids = vec![];
        let mut at = 0;
        parse(&mut solver, layout.as_bytes(), &mut at, None, 0, &mut ids, args[1].as_str().unwrap().trim_start_matches('%').parse().unwrap());
        assert_eq!(at, layout.len());
        solver.solve();
        for r in &args[5..] {
            let (pane, width) = r.as_str().unwrap().split_once('=').unwrap();
            let leaf = ids.iter().position(|id| *id == pane.parse::<i64>().unwrap()).unwrap();
            rail(&mut solver, leaf, width.parse().unwrap());
        }
        solver.positions(0);
        let text = emit(&solver, &ids, 0);
        let checksum = text.bytes().fold(0u16, |acc, b| acc.rotate_right(1).wrapping_add(b as u16));
        assert_eq!(format!("{checksum:04x},{text}"), case["expected"].as_str().unwrap(), "case {}", i + 1);
    }
}

fn row(count: usize, axis: Axis) -> Node {
    Node::container("root", axis, (0..count).map(|i| Node::block(i.to_string(), crate::layout::Content::Terminal { terminal: i.to_string() })).collect())
}

fn weights(node: &Node) -> Vec<f32> {
    node.children().iter().map(|c| match c.size { Some(Size::Weight { weight }) => weight, _ => 1.0 }).collect()
}

fn shape(text: &str) -> Node {
    fn next(text: &[u8], at: &mut usize) -> Node {
        let start = *at;
        let token = text[*at]; *at += 1;
        if token.is_ascii_alphabetic() {
            return Node::block((token as char).to_string(), crate::layout::Content::Terminal { terminal: (token as char).to_string() });
        }
        let close = if token == b'{' { b'}' } else { b']' };
        let mut children = vec![];
        while text[*at] != close { children.push(next(text, at)); }
        *at += 1;
        Node::container(format!("group{start}"), if token == b'{' { Axis::Row } else { Axis::Column }, children)
    }
    next(text.as_bytes(), &mut 0)
}

fn spans(root: &Node, target: &str) -> BTreeMap<String, [i64; 2]> {
    let mut solver = Solver::new([23, 8]);
    build(&mut solver, root, target, [188, 49], [0, 0], None, 0, [1.0, 1.0]);
    let ids: Vec<_> = root.panes().iter().map(|n| n.id.clone()).collect();
    solver.solve();
    ids.into_iter().zip(solver.leaves.iter().map(|&id| solver.nodes[id].size)).collect()
}

#[test]
fn repeated_split_operations_preserve_symmetry_and_visible_decay() {
    for (axis, direction, delta) in [(0, Axis::Row, 3..=4), (1, Axis::Column, 1..=2)] {
        for left_nested in [true, false] {
            let mut root = Node::container("root", direction, vec![shape("A")]);
            let (first, second, target) = if left_nested { ("C", "B", "A") } else { ("B", "C", "B") };
            root.split("A", direction, shape(first), "split1".into());
            root.split(target, direction, shape(second), "split2".into());
            let left = spans(&root, "A"); let right = spans(&root, "C"); let center = spans(&root, "B");
            assert!((left["A"][axis] - right["C"][axis]).abs() <= 1, "{direction:?}: {left:?} {right:?}");
            assert!((left["B"][axis] - right["B"][axis]).abs() <= 1);
            assert!((left["C"][axis] - right["A"][axis]).abs() <= 1);
            assert!(delta.contains(&(left["B"][axis] - left["C"][axis])), "{direction:?}: {left:?}");
            assert!((center["A"][axis] - center["C"][axis]).abs() <= 1);
            assert!(center["B"][axis] > center["A"][axis]);
        }
    }
}

fn fixture_spans(layout: &str, target: &str) -> (BTreeMap<String, [i64; 2]>, String) {
    let (width, height) = layout.split(',').next().unwrap().split_once('x').unwrap();
    let minimum = [(width.parse::<i64>().unwrap() / 8).clamp(20, 28), (height.parse::<i64>().unwrap() / 6).clamp(6, 10)];
    let mut solver = Solver::new(minimum);
    let mut ids = vec![];
    parse(&mut solver, layout.as_bytes(), &mut 0, None, 0, &mut ids, (target.as_bytes()[0] - b'A') as i64);
    for _ in 0..6 {
        let before = emit(&solver, &ids, 0);
        solver.solve();
        let after = emit(&solver, &ids, 0);
        if before == after { break; }
        let mut next = Solver::new(minimum);
        ids.clear();
        parse(&mut next, after.as_bytes(), &mut 0, None, 0, &mut ids, (target.as_bytes()[0] - b'A') as i64);
        solver = next;
    }
    (solver.leaves.iter().map(|&id| (((ids[id] as u8 + b'A') as char).to_string(), solver.nodes[id].size)).collect(), emit(&solver, &ids, 0))
}

#[test]
fn grouped_branches_keep_directionality_and_history_independence() {
    for (pattern, axis, first, a, b, second, c, d) in [
        ("188x49,0,0{66x49,0,0,0,121x49,67,0[121x16,67,0,1,121x13,67,17,2,121x18,67,31,3]}", 1, "B", "C", "D", "D", "C", "B"),
        ("188x49,0,0{66x49,0,0,0,121x49,67,0[121x30,67,0,3,121x18,67,31[121x9,67,31,1,121x8,67,41,2]]}", 1, "B", "C", "D", "D", "C", "B"),
        ("188x49,0,0[188x17,0,0,0,188x31,0,18{72x31,0,18,3,115x31,73,18{57x31,73,18,1,57x31,131,18,2}}]", 0, "B", "C", "D", "D", "C", "B"),
        ("188x54,0,0{84x54,0,0,0,103x54,85,0{38x54,85,0[38x29,85,0,1,38x24,85,30,3],64x54,124,0,2}}", 0, "A", "B", "C", "C", "B", "A"),
    ] {
        let (first, next) = fixture_spans(pattern, first); let (second, _) = fixture_spans(&next, second);
        assert!(first[a][axis] > first[b][axis], "{pattern}: {first:?}");
        assert!(second[c][axis] > second[d][axis], "{pattern}: {second:?}");
    }
    for pattern in ["{A[BC][DE]}", "{A[BCD]}", "{A[D[BC]]}", "[A{D{BC}}]", "{A[{BC}{DE}]}"] {
        let root = shape(pattern);
        let mut growth = Growth::default();
        let sequence: Vec<_> = ["A", "C", "B", "C", "B"].into_iter().map(|target| growth.derive("t", &root, target, [1880.0, 980.0], [10.0, 20.0], true, false)).collect();
        assert_eq!(sequence[1], sequence[3], "{pattern}");
        assert_eq!(sequence[2], sequence[4], "{pattern}");
    }
}

#[test]
fn contested_group_matches_the_actual_tmux_readback_and_directionality() {
    let fixture = "188x47,0,0{40x47,0,0,0,147x47,41,0[147x23,41,0{54x23,41,0,1,92x23,96,0,3},147x23,41,24{76x23,41,24,2,70x23,118,24,4}]}";
    let (first, next) = fixture_spans(fixture, "C");
    assert!(first["D"][0] > first["E"][0]);
    let (second, _) = fixture_spans(&next, "E");
    assert_eq!((first["D"][0], first["E"][0]), (75, 53));
    assert_eq!((second["D"][0], second["C"][0]), (75, 54));
}

#[test]
fn native_focus_corridors_decay_and_remain_symmetric() {
    for axis in [Axis::Row, Axis::Column] {
        for count in [3, 5] {
            let root = row(count, axis);
            let first = derive(&root, "0", [1880.0, 980.0], [10.0, 20.0]);
            let last = derive(&root, &(count - 1).to_string(), [1880.0, 980.0], [10.0, 20.0]);
            let a = weights(&first);
            let mut b = weights(&last); b.reverse();
            assert!(a.windows(2).all(|pair| pair[0] > pair[1]), "{axis:?} {count}: {a:?}");
            assert!(a.iter().zip(&b).all(|(a, b)| (a - b).abs() <= 1.0), "{axis:?}: {a:?} {b:?}");
            let center = weights(&derive(&root, &(count / 2).to_string(), [1880.0, 980.0], [10.0, 20.0]));
            assert!(center.iter().zip(center.iter().rev()).all(|(a, b)| (a - b).abs() <= 1.0));
            assert!(center[count / 2] > center[0]);
        }
    }
}

#[test]
fn native_growth_is_local_cached_and_history_independent() {
    let root = row(3, Axis::Row);
    let original = root.clone();
    let mut a = Growth::default();
    let mut b = Growth::default();
    let mut results = vec![];
    for target in ["0", "2", "1", "2", "1"] {
        results.push(a.derive("t", &root, target, [1880.0, 980.0], [10.0, 20.0], true, false));
    }
    assert_eq!(results[1], results[3]);
    assert_eq!(results[2], results[4]);
    let independent = b.derive("t", &root, "0", [1880.0, 980.0], [10.0, 20.0], true, false);
    assert_eq!(weights(&independent), [87.0, 51.0, 48.0]);
    assert_ne!(independent, results[4]);
    let again = a.derive("t", &root, "1", [1880.0, 980.0], [10.0, 20.0], true, false);
    assert!(Rc::ptr_eq(&again, &results[4]));
    assert_eq!(a.solves, 5);
    assert_eq!(root, original);
    assert_eq!(root.children().iter().map(|c| &c.id).collect::<Vec<_>>(), again.children().iter().map(|c| &c.id).collect::<Vec<_>>());
}

#[test]
fn toggles_suspension_zoom_and_manual_resize() {
    let root = row(3, Axis::Row);
    let mut growth = Growth::default();
    let area = [1880.0, 980.0]; let cell = [10.0, 20.0];
    let grown = growth.derive("t", &root, "0", area, cell, true, false);
    assert_eq!(growth.derive("t", &root, "2", area, cell, false, false), grown);
    assert_eq!(growth.derive("t", &root, "2", [2400.0, 1400.0], cell, false, false), grown);
    assert_eq!(*growth.derive("t", &root.children()[0], "0", area, cell, false, true), root.children()[0]);
    assert_eq!(growth.derive("t", &root, "2", area, cell, false, false), grown);
    growth.preferences.entry("t".into()).or_default().suspended = true;
    growth.resume("t");
    assert_eq!(*growth.derive("t", &root, "0", area, cell, true, false), root);
    growth.preferences.get_mut("t").unwrap().suspended = false;
    growth.resume("t");
    assert_ne!(*growth.derive("t", &root, "0", area, cell, true, false), root);
    growth.manual("t", &root, "0");
    assert_eq!(*growth.derive("t", &root, "0", area, cell, true, false), root);
    assert_ne!(*growth.derive("t", &root, "1", area, cell, true, false), root);
    assert_eq!(*growth.derive("t", &root.children()[0], "0", area, cell, true, true), root.children()[0]);
}

#[test]
#[ignore]
fn benchmark_focus_growth() {
    use std::hint::black_box;
    use std::time::Instant;
    let iterations = std::env::var("FOCUS_GROW_BENCH_ITERATIONS").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(10000);
    for count in [3, 9, 81] {
        let root = if count == 3 { row(3, Axis::Row) } else {
            Node::container("root", Axis::Row, (0..count / 3).map(|i| {
                let mut n = row(3, Axis::Column); n.id = format!("col{i}");
                for child in n.children_mut().unwrap() { child.id = format!("{i}-{}", child.id); }
                n
            }).collect())
        };
        let ids: Vec<_> = root.panes().iter().map(|n| n.id.clone()).collect();
        let area = [3840.0, 2160.0]; let cell = [10.0, 20.0];
        let start = Instant::now();
        for i in 0..iterations { black_box(derive(black_box(&root), &ids[i % ids.len()], area, cell)); }
        let pure = start.elapsed().as_nanos() as f64 / iterations as f64;
        let mut growth = Growth::default();
        let start = Instant::now();
        for i in 0..iterations { black_box(growth.derive("t", black_box(&root), &ids[i % ids.len()], area, cell, true, false)); }
        let changed = start.elapsed().as_nanos() as f64 / iterations as f64;
        let last = &ids[(iterations - 1) % ids.len()];
        let calls = growth.solves;
        let start = Instant::now();
        for _ in 0..iterations { black_box(growth.derive("t", black_box(&root), last, area, cell, true, false)); }
        let cached = start.elapsed().as_nanos() as f64 / iterations as f64;
        assert_eq!(growth.solves, calls);
        eprintln!("panes={count} iterations={iterations} pure_ns={pure:.0} changed_ns={changed:.0} cached_ns={cached:.0} changed_solves={calls} cached_solves={}", growth.solves - calls);
    }
}

#[test]
fn geometry_and_topology_changes_invalidate_only_the_affected_tab() {
    let mut root = row(3, Axis::Row);
    let mut growth = Growth::default();
    let area = [1880.0, 980.0]; let cell = [10.0, 20.0];
    growth.derive("t", &root, "0", area, cell, true, false);
    growth.derive("other", &root, "1", area, cell, true, false);
    let other = growth.displayed("other").unwrap();
    growth.derive("t", &root, "0", [2400.0, 1400.0], cell, true, false);
    growth.derive("t", &root, "0", [2400.0, 1400.0], [12.0, 24.0], true, false);
    root.children_mut().unwrap()[2].hidden = true;
    let hidden = growth.derive("t", &root, "0", area, cell, true, false);
    assert_eq!(hidden.children()[2], root.children()[2]);
    assert!(Rc::ptr_eq(&other, &growth.displayed("other").unwrap()));
    assert_eq!(growth.solves, 5);
    growth.retain(|id| id == "other");
    assert!(growth.displayed("t").is_none());
    assert!(growth.displayed("other").is_some());
}

#[test]
fn single_panes_zoom_and_early_manual_resize_skip_growth() {
    let mut growth = Growth::default();
    let root = row(1, Axis::Row);
    let area = [1880.0, 980.0]; let cell = [10.0, 20.0];
    assert_eq!(*growth.derive("single", &root, "0", area, cell, true, false), root);
    assert_eq!(growth.solves, 0);
    let root = row(3, Axis::Row);
    growth.manual("early", &root, "0");
    assert_eq!(*growth.derive("early", &root, "0", area, cell, true, false), root);
    assert_eq!(growth.solves, 0);
    growth.resume("early");
    assert_ne!(*growth.derive("early", &root, "0", area, cell, true, false), root);
    let before = growth.solves;
    assert_eq!(*growth.derive("early", &root.children()[0], "0", area, cell, true, true), root.children()[0]);
    assert_eq!(growth.solves, before);
}
