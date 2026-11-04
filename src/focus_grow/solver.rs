#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind { Leaf, X, Y }

impl Kind {
    fn axis(self) -> usize { usize::from(self != Self::X) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_and_disconnected_corridors_keep_exact_distances_and_pressure() {
        for axis in 0..2 {
            for focus in [0usize, 75, 149] {
                let mut solver = Solver::new([23, 8]);
                let kind = if axis == 0 { Kind::X } else { Kind::Y };
                solver.nodes.push(Node::new([10000; 2], [0; 2], kind, None, 0));
                for index in 0..151 {
                    let mut size = [10; 2];
                    size[axis] = 2;
                    let mut pos = [0; 2];
                    pos[axis] = if index == 150 { 1000 } else { index as i64 * 3 };
                    let id = solver.nodes.len();
                    solver.nodes.push(Node::new(size, pos, Kind::Leaf, Some(0), index));
                    solver.nodes[0].children.push(id);
                    solver.leaves.push(id);
                }
                solver.focus = solver.leaves[focus];
                solver.corridor = [0, 0, 10000, 10000];
                solver.graph();
                for (index, &id) in solver.leaves.iter().enumerate() {
                    let distance = if index == 150 { -1 } else { index.abs_diff(focus) as i64 };
                    assert_eq!(solver.nodes[id].distance, distance);
                    for pressure_axis in 0..2 {
                        let mut expected = 10000;
                        for _ in 0..if distance < 0 { 9999 } else { distance } {
                            expected = (expected * if pressure_axis == 0 { 90 } else { 70 } / 100).max(1);
                        }
                        if id == solver.focus { expected = expected * if pressure_axis == 0 { 200 } else { 140 } / 100; }
                        assert_eq!(solver.leaf_pressure(id, pressure_axis), expected);
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub(super) struct Node {
    pub size: [i64; 2],
    pub pos: [i64; 2],
    pub kind: Kind,
    pub parent: Option<usize>,
    pub index: usize,
    pub children: Vec<usize>,
    pub(super) contains: bool,
    target: Option<usize>,
    pressure: [i64; 2],
    distance: i64,
}

impl Node {
    pub fn new(size: [i64; 2], pos: [i64; 2], kind: Kind, parent: Option<usize>, index: usize) -> Self {
        Self { size, pos, kind, parent, index, children: vec![], contains: false, target: None, pressure: [0; 2], distance: -1 }
    }
}

pub(super) struct Solver {
    pub nodes: Vec<Node>,
    pub leaves: Vec<usize>,
    pub focus: usize,
    corridor: [i64; 4],
    minimum: [i64; 2],
}

fn overlap(a: i64, b: i64, c: i64, d: i64) -> bool { a < d && c < b }

impl Solver {
    pub fn new(minimum: [i64; 2]) -> Self {
        Self { nodes: vec![], leaves: vec![], focus: usize::MAX, corridor: [0; 4], minimum }
    }

    pub fn solve(&mut self) {
        let f = &self.nodes[self.focus];
        self.corridor = [f.pos[0], f.pos[1], f.pos[0] + f.size[0], f.pos[1] + f.size[1]];
        let mut child = self.focus;
        self.nodes[child].contains = true;
        while let Some(parent) = self.nodes[child].parent {
            self.nodes[parent].contains = true;
            self.nodes[parent].target = Some(self.nodes[child].index);
            child = parent;
        }
        self.graph();
        if self.vertical() { return; }
        self.annotate(0);
        let mut id = 0;
        loop {
            self.rebalance(id);
            let Some(t) = self.nodes[id].target else { break };
            id = self.nodes[id].children[t];
        }
        self.fit(0);
        self.positions(0);
    }

    fn graph(&mut self) {
        self.nodes[self.focus].distance = 0;
        let mut queue = Vec::with_capacity(self.leaves.len());
        queue.push(self.focus);
        let mut head = 0;
        while head < queue.len() {
            let id = queue[head];
            head += 1;
            let pos = self.nodes[id].pos;
            let size = self.nodes[id].size;
            let distance = self.nodes[id].distance + 1;
            for &neighbor in &self.leaves {
                if self.nodes[neighbor].distance >= 0 { continue; }
                let b = &self.nodes[neighbor];
                let adjacent = (0..2).any(|axis| {
                    let other = 1 - axis;
                    overlap(pos[other], pos[other] + size[other], b.pos[other], b.pos[other] + b.size[other])
                        && (pos[axis] + size[axis] + 1 == b.pos[axis]
                            || b.pos[axis] + b.size[axis] + 1 == pos[axis])
                });
                if !adjacent { continue; }
                self.nodes[neighbor].distance = distance;
                queue.push(neighbor);
            }
        }
    }

    fn leaf_pressure(&self, id: usize, axis: usize) -> i64 {
        let n = &self.nodes[id];
        let other = 1 - axis;
        if !overlap(n.pos[other], n.pos[other] + n.size[other], self.corridor[other], self.corridor[other + 2]) { return 0; }
        let distance = if n.distance < 0 { 9999 } else { n.distance };
        let mut score = 10000;
        for _ in 0..distance {
            score = (score * if axis == 0 { 90 } else { 70 } / 100).max(1);
            if score == 1 { break; }
        }
        if id == self.focus { score = score * if axis == 0 { 200 } else { 140 } / 100; }
        score
    }

    fn annotate(&mut self, id: usize) {
        for index in 0..self.nodes[id].children.len() {
            self.annotate(self.nodes[id].children[index]);
        }
        for axis in 0..2 {
            let children = &self.nodes[id].children;
            self.nodes[id].pressure[axis] = if children.is_empty() { self.leaf_pressure(id, axis) }
                else { children.iter().map(|&c| self.nodes[c].pressure[axis]).max().unwrap() };
        }
    }

    fn pressure(&self, id: usize, axis: usize) -> i64 {
        if self.nodes[id].kind == Kind::Leaf { return self.leaf_pressure(id, axis); }
        self.nodes[id].children.iter().map(|&c| self.pressure(c, axis)).sum()
    }

    fn contains_axis(&self, id: usize, axis: usize) -> bool {
        let n = &self.nodes[id];
        n.kind != Kind::Leaf && (n.kind.axis() == axis || n.children.iter().any(|&c| self.contains_axis(c, axis)))
    }

    fn weight(&self, child: usize, axis: usize, index: usize, target: usize) -> i64 {
        let n = &self.nodes[child];
        if index == target && n.kind != Kind::Leaf {
            let pressure = self.pressure(child, axis);
            let weight = pressure + if self.contains_axis(child, axis) { pressure } else if axis == 1 { (pressure + 1) / 2 } else { 0 };
            if weight > 0 { return weight; }
        }
        if index != target {
            let p = self.pressure(child, axis);
            if p > 0 { return p; }
        }
        n.pressure[axis].max(1)
    }

    pub fn allocate(&self, axis: usize, available: i64, weights: &[i64]) -> Vec<i64> {
        let count = weights.len() as i64;
        let mut minimum = self.minimum[axis];
        if minimum * count > available { minimum = (available / count).max(1); }
        minimum = minimum.max(if axis == 0 { 8 } else { 3 });
        if minimum * count > available { minimum = available / count; }
        minimum = minimum.max(1);
        let extra = (available - minimum * count).max(0);
        let total: i64 = weights.iter().sum();
        let uniform = total <= 0;
        let total = if uniform { count } else { total };
        let weight = |index: usize| if uniform { 1 } else { weights[index] };
        let mut sizes: Vec<_> = (0..weights.len()).map(|i| minimum + weight(i) * extra / total).collect();
        let mut used: i64 = sizes.iter().sum();
        if used < available {
            let mut best = 0;
            for i in 1..weights.len() { if weight(i) > weight(best) { best = i; } }
            sizes[best] += available - used;
            used = available;
        }
        while used > available {
            let mut best: Option<usize> = None;
            for i in 0..weights.len() {
                if sizes[i] > minimum && best.is_none_or(|b| weight(i) < weight(b)) { best = Some(i); }
            }
            let Some(best) = best else { break };
            sizes[best] -= 1;
            used -= 1;
        }
        sizes
    }

    pub(super) fn positions(&mut self, id: usize) {
        let axis = self.nodes[id].kind.axis();
        let mut cursor = self.nodes[id].pos[axis];
        for index in 0..self.nodes[id].children.len() {
            let child = self.nodes[id].children[index];
            self.nodes[child].pos[axis] = cursor;
            self.nodes[child].pos[1 - axis] = self.nodes[id].pos[1 - axis];
            self.nodes[child].size[1 - axis] = self.nodes[id].size[1 - axis];
            self.positions(child);
            cursor += self.nodes[child].size[axis] + 1;
        }
    }

    fn rebalance(&mut self, id: usize) {
        let n = &self.nodes[id];
        let Some(target) = n.target else { return };
        let count = n.children.len();
        let axis = n.kind.axis();
        let available = n.size[axis] - (count as i64 - 1);
        if count <= 1 || available <= 0 { return; }
        let weights: Vec<_> = n.children.iter().enumerate().map(|(i, &c)| self.weight(c, axis, i, target)).collect();
        let sizes = self.allocate(axis, available, &weights);
        for i in 0..count {
            let child = self.nodes[id].children[i];
            self.nodes[child].size[axis] = sizes[i];
            self.nodes[child].size[1 - axis] = self.nodes[id].size[1 - axis];
        }
        self.positions(id);
    }

    fn fit(&mut self, id: usize) {
        let n = &self.nodes[id];
        let count = n.children.len();
        if count == 0 { return; }
        let axis = n.kind.axis();
        let available = n.size[axis] - (count as i64 - 1);
        if available <= 0 { return; }
        let side = n.parent.and_then(|p| {
            let parent = &self.nodes[p];
            parent.target.filter(|&t| parent.kind.axis() == axis && t != n.index).map(|t| n.index > t)
        });
        let boundary = self.nodes[id].children[if side == Some(true) { 0 } else { count - 1 }];
        let grouped = count > 1 && self.nodes[boundary].kind == if axis == 0 { Kind::Y } else { Kind::X };
        let mut weights = Vec::with_capacity(count);
        for index in 0..count {
            let child = self.nodes[id].children[index];
            self.nodes[child].size[1 - axis] = self.nodes[id].size[1 - axis];
            let c = &self.nodes[child];
            let mut weight = if c.pressure[axis] > 0 { c.pressure[axis] } else { c.size[axis].max(1) };
            if let Some(near_first) = side {
                if grouped {
                    let p = self.pressure(child, axis);
                    if p > 0 { weight = p; }
                } else {
                    weight = if near_first { c.pos[axis] + c.size[axis] - self.nodes[id].pos[axis] }
                        else { self.nodes[id].pos[axis] + self.nodes[id].size[axis] - c.pos[axis] };
                    weight = weight.max(1);
                }
            }
            weights.push(weight);
        }
        if self.nodes[id].target.is_none()
            && (self.nodes[id].children.iter().map(|&c| self.nodes[c].size[axis]).sum::<i64>() != available || weights.iter().sum::<i64>() > 0)
        {
            let sizes = self.allocate(axis, available, &weights);
            for (i, size) in sizes.into_iter().enumerate() {
                let child = self.nodes[id].children[i];
                self.nodes[child].size[axis] = size;
            }
        }
        for index in 0..count { self.fit(self.nodes[id].children[index]); }
    }

    fn heights(&mut self, id: usize) {
        if self.nodes[id].children.is_empty() { return; }
        for index in 0..self.nodes[id].children.len() { self.heights(self.nodes[id].children[index]); }
        let children = &self.nodes[id].children;
        self.nodes[id].size[1] = children.iter().map(|&c| self.nodes[c].size[1]).sum::<i64>() + children.len() as i64 - 1;
    }

    fn vertical(&mut self) -> bool {
        if self.leaves.len() < 5
            || !self.leaves.iter().any(|&c| self.nodes[c].pos[1] != self.nodes[0].pos[1] || self.nodes[c].size[1] != self.nodes[0].size[1])
            || self.leaves.iter().any(|&c| self.nodes[c].pos[0] != self.nodes[0].pos[0] || self.nodes[c].size[0] != self.nodes[0].size[0])
        { return false; }
        let available = self.nodes[0].size[1] - (self.leaves.len() as i64 - 1);
        if available <= 0 { return false; }
        let max = self.leaves.iter().map(|&c| self.nodes[c].distance).max().unwrap();
        let tiers: i64 = self.leaves.iter().map(|&c| max - self.nodes[c].distance).sum();
        let count = self.leaves.len() as i64;
        let mut minimum = self.minimum[1].min((available - tiers) / count);
        if minimum < 3 && 3 * count + tiers <= available { minimum = 3; }
        minimum = minimum.max(1);
        for &c in &self.leaves { self.nodes[c].size[1] = minimum + max - self.nodes[c].distance; }
        if available > self.leaves.iter().map(|&c| self.nodes[c].size[1]).sum() { self.nodes[self.focus].size[1] += 1; }
        self.heights(0);
        self.positions(0);
        true
    }
}
