#[path = "focus_grow/solver.rs"]
mod solver;

use crate::layout::{Axis, Node, Size};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preference {
    pub enabled: bool,
    pub suspended: bool,
}

impl Default for Preference {
    fn default() -> Self { Self { enabled: true, suspended: false } }
}

#[derive(Clone, PartialEq)]
struct Input {
    root: Node,
    target: String,
    area: [f32; 2],
    cell: [f32; 2],
}

struct Entry {
    input: Input,
    output: Rc<Node>,
    manual_target: Option<String>,
    active: bool,
}

#[derive(Default)]
pub struct Growth {
    pub preferences: BTreeMap<String, Preference>,
    entries: BTreeMap<String, Entry>,
    pub solves: u64,
}

impl Growth {
    pub fn derive(&mut self, tab: &str, root: &Node, target: &str, area: [f32; 2], cell: [f32; 2], enabled: bool, zoomed: bool) -> Rc<Node> {
        if zoomed { return Rc::new(root.clone()); }
        let preference = self.preferences.get(tab).cloned().unwrap_or_default();
        let active = enabled && preference.enabled && !preference.suspended && !zoomed;
        let mut same_root = false;
        let mut manual = false;
        if let Some(entry) = self.entries.get_mut(tab) {
            same_root = entry.input.root == *root;
            if same_root && !active && !zoomed {
                entry.active = false;
                return entry.output.clone();
            }
            let same_geometry = same_root && entry.input.area == area && entry.input.cell == cell;
            manual = entry.manual_target.as_deref() == Some(target);
            if same_geometry && ((!active && !zoomed) || (entry.active == active && entry.input.target == target) || manual) {
                entry.active = active;
                return entry.output.clone();
            }
        }
        let output = if active && !manual && root.panes().len() > 1 {
            self.solves += 1;
            derive(root, target, area, cell)
        } else { root.clone() };
        let output = Rc::new(output);
        if let Some(entry) = self.entries.get_mut(tab) {
            if !same_root { entry.input.root = root.clone(); }
            entry.input.target.clear();
            entry.input.target.push_str(target);
            entry.input.area = area;
            entry.input.cell = cell;
            entry.output = output.clone();
            if !manual { entry.manual_target = None; }
            entry.active = active;
        } else {
            self.entries.insert(tab.to_owned(), Entry {
                input: Input { root: root.clone(), target: target.to_owned(), area, cell }, output: output.clone(),
                manual_target: None,
                active,
            });
        }
        output
    }

    pub fn resume(&mut self, tab: &str) { self.entries.remove(tab); }

    pub fn retain(&mut self, mut exists: impl FnMut(&str) -> bool) { self.entries.retain(|id, _| exists(id)); }

    pub fn manual(&mut self, tab: &str, root: &Node, target: &str) {
        if let Some(entry) = self.entries.get_mut(tab) {
            entry.input.root = root.clone();
            entry.output = Rc::new(root.clone());
            entry.manual_target = Some(target.to_owned());
        } else {
            self.entries.insert(tab.to_owned(), Entry {
                input: Input { root: root.clone(), target: target.to_owned(), area: [0.0; 2], cell: [1.0; 2] },
                output: Rc::new(root.clone()), manual_target: Some(target.to_owned()), active: false,
            });
        }
    }

    pub fn displayed(&self, tab: &str) -> Option<Rc<Node>> { self.entries.get(tab).map(|e| e.output.clone()) }
}

pub fn derive(root: &Node, target: &str, area: [f32; 2], cell: [f32; 2]) -> Node {
    let mut output = root.clone();
    for _ in 0..6 {
        if !derive_once(&mut output, target, area, cell) { break; }
    }
    output
}

fn derive_once(root: &mut Node, target: &str, area: [f32; 2], cell: [f32; 2]) -> bool {
    let size = [0, 1].map(|axis| (area[axis] / cell[axis].max(1.0)).floor().max(1.0) as i64);
    let minimum = [(size[0] / 8).clamp(20, 28), (size[1] / 6).clamp(6, 10)];
    let mut solver = solver::Solver::new(minimum);
    build(&mut solver, root, target, size, [0; 2], None, 0, cell);
    if solver.focus == usize::MAX || solver.leaves.len() <= 1 { return false; }
    solver.solve();
    apply(root, &solver, 0)
}

fn build(solver: &mut solver::Solver, node: &Node, target: &str, size: [i64; 2], pos: [i64; 2], parent: Option<usize>, index: usize, cell: [f32; 2]) -> usize {
    let id = solver.nodes.len();
    let kind = match node.axis() { Some(Axis::Row) => solver::Kind::X, Some(Axis::Column) => solver::Kind::Y, None => solver::Kind::Leaf };
    solver.nodes.push(solver::Node::new(size, pos, kind, parent, index));
    if kind == solver::Kind::Leaf {
        solver.leaves.push(id);
        if node.id == target || node.find(target).is_some() { solver.focus = id; }
        return id;
    }
    let axis = usize::from(kind == solver::Kind::Y);
    let children: Vec<_> = node.children().iter().filter(|n| !n.hidden).collect();
    let available = (size[axis] - children.len().saturating_sub(1) as i64).max(children.len() as i64);
    let fixed = children.iter().filter_map(|n| match n.size { Some(Size::Px { px }) => Some((px / cell[axis]).max(1.0)), _ => None }).sum::<f32>();
    let weight = |n: &Node| match n.size { Some(Size::Px { .. }) => 0.0, Some(Size::Weight { weight }) => weight.max(0.001), None => 1.0 };
    let total = children.iter().map(|n| weight(n)).sum::<f32>().max(f32::EPSILON);
    let desired: Vec<_> = children.iter().map(|n| match n.size { Some(Size::Px { px }) => (px / cell[axis]).max(1.0), _ => (available as f32 - fixed).max(0.0) * weight(n) / total }).collect();
    let residual = (available - children.len() as i64).max(0);
    let total = desired.iter().map(|s| (s - 1.0).max(0.0)).sum::<f32>().max(f32::EPSILON);
    let mut sizes: Vec<i64> = desired.iter().map(|s| 1 + (residual as f32 * (s - 1.0).max(0.0) / total).floor() as i64).collect();
    let remainder = available - sizes.iter().sum::<i64>();
    if let Some(first) = sizes.first_mut() { *first += remainder; }
    let mut cursor = pos;
    for (index, (child, span)) in children.iter().zip(sizes).enumerate() {
        let mut child_size = size;
        child_size[axis] = span;
        let child = build(solver, child, target, child_size, cursor, Some(id), index, cell);
        solver.nodes[id].children.push(child);
        cursor[axis] += span + 1;
    }
    id
}

fn apply(node: &mut Node, solver: &solver::Solver, id: usize) -> bool {
    let Some(axis) = node.axis() else { return false };
    let axis = usize::from(axis == Axis::Column);
    let Some(children) = node.children_mut() else { return false };
    let mut changed = false;
    for (child, &index) in children.iter_mut().filter(|n| !n.hidden).zip(&solver.nodes[id].children) {
        if !matches!(child.size, Some(Size::Px { .. })) {
            let size = Some(Size::Weight { weight: solver.nodes[index].size[axis] as f32 });
            changed |= child.size != size;
            child.size = size;
        }
        changed |= apply(child, solver, index);
    }
    changed
}

#[cfg(test)]
#[path = "focus_grow/tests.rs"]
mod tests;
