#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind { Leaf, X, Y }

impl Kind {
    fn axis(self) -> usize { usize::from(self != Self::X) }
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
        let mut edges = vec![vec![]; self.nodes.len()];
        for (i, &a) in self.leaves.iter().enumerate() {
            for &b in &self.leaves[i + 1..] {
                let (a_node, b_node) = (&self.nodes[a], &self.nodes[b]);
                if (0..2).any(|axis| {
                    let other = 1 - axis;
                    overlap(a_node.pos[other], a_node.pos[other] + a_node.size[other], b_node.pos[other], b_node.pos[other] + b_node.size[other])
                        && (a_node.pos[axis] + a_node.size[axis] + 1 == b_node.pos[axis]
                            || b_node.pos[axis] + b_node.size[axis] + 1 == a_node.pos[axis])
                }) {
                    edges[a].push(b);
                    edges[b].push(a);
                }
            }
        }
        self.nodes[self.focus].distance = 0;
        let mut queue = vec![self.focus];
        let mut head = 0;
        while head < queue.len() {
            let id = queue[head];
            head += 1;
            for &neighbor in &edges[id] {
                if self.nodes[neighbor].distance >= 0 { continue; }
                self.nodes[neighbor].distance = self.nodes[id].distance + 1;
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
        for _ in 0..distance { score = (score * if axis == 0 { 90 } else { 70 } / 100).max(1); }
        if id == self.focus { score = score * if axis == 0 { 200 } else { 140 } / 100; }
        score
    }

    fn annotate(&mut self, id: usize) {
        let children = self.nodes[id].children.clone();
        for &child in &children { self.annotate(child); }
        for axis in 0..2 {
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
        let mut weights = weights.to_vec();
        let mut total: i64 = weights.iter().sum();
        if total <= 0 { weights.fill(1); total = count; }
        let mut sizes: Vec<_> = weights.iter().map(|w| minimum + w * extra / total).collect();
        let mut used: i64 = sizes.iter().sum();
        while used < available {
            let mut best = 0;
            for i in 1..weights.len() { if weights[i] > weights[best] { best = i; } }
            sizes[best] += 1;
            used += 1;
        }
        while used > available {
            let mut best: Option<usize> = None;
            for i in 0..weights.len() {
                if sizes[i] > minimum && best.is_none_or(|b| weights[i] < weights[b]) { best = Some(i); }
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
        for child in self.nodes[id].children.clone() {
            self.nodes[child].pos[axis] = cursor;
            self.nodes[child].pos[1 - axis] = self.nodes[id].pos[1 - axis];
            self.nodes[child].size[1 - axis] = self.nodes[id].size[1 - axis];
            self.positions(child);
            cursor += self.nodes[child].size[axis] + 1;
        }
    }

    fn rebalance(&mut self, id: usize) {
        let n = &self.nodes[id];
        let children = n.children.clone();
        let Some(target) = n.target else { return };
        let axis = n.kind.axis();
        let available = n.size[axis] - (children.len() as i64 - 1);
        if children.len() <= 1 || available <= 0 { return; }
        let weights: Vec<_> = children.iter().enumerate().map(|(i, &c)| self.weight(c, axis, i, target)).collect();
        let sizes = self.allocate(axis, available, &weights);
        for (i, &child) in children.iter().enumerate() {
            self.nodes[child].size[axis] = sizes[i];
            self.nodes[child].size[1 - axis] = self.nodes[id].size[1 - axis];
        }
        self.positions(id);
    }

    fn fit(&mut self, id: usize) {
        let n = &self.nodes[id];
        let children = n.children.clone();
        if children.is_empty() { return; }
        let axis = n.kind.axis();
        let available = n.size[axis] - (children.len() as i64 - 1);
        if available <= 0 { return; }
        let side = n.parent.and_then(|p| {
            let parent = &self.nodes[p];
            parent.target.filter(|&t| parent.kind.axis() == axis && t != n.index).map(|t| n.index > t)
        });
        let boundary = if side == Some(true) { children[0] } else { *children.last().unwrap() };
        let grouped = children.len() > 1 && self.nodes[boundary].kind == if axis == 0 { Kind::Y } else { Kind::X };
        let mut weights = vec![];
        for &child in &children {
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
            && (children.iter().map(|&c| self.nodes[c].size[axis]).sum::<i64>() != available || weights.iter().sum::<i64>() > 0)
        {
            let sizes = self.allocate(axis, available, &weights);
            for (i, &child) in children.iter().enumerate() { self.nodes[child].size[axis] = sizes[i]; }
        }
        for child in children { self.fit(child); }
    }

    fn heights(&mut self, id: usize) {
        let children = self.nodes[id].children.clone();
        if children.is_empty() { return; }
        for &c in &children { self.heights(c); }
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
