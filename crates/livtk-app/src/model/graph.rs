//! The graph: every node of the project and the links between them, laid out so dependencies
//! read at a glance, with a camera to pan and zoom. Pure geometry; the view only draws it.
//!
//! Linked nodes are laid out in layers, as dependency graphs read best: a node sits one column
//! right of everything it comes after (its dependencies, what it was learned from, what it
//! supersedes), and the order within each column is swept to cut crossings. Each connected part
//! is its own block; nodes with no links sit in a grid below.

use std::collections::{HashMap, HashSet};

use livtk_core::{Kind, Rel, State, Summary};
use uuid::Uuid;

use super::Model;

/// A card's size in world units, which are pixels at zoom 1.
pub const CARD_W: f32 = 232.;
pub const CARD_H: f32 = 36.;
/// Between columns, which the arrows cross.
const COL_GAP: f32 = 88.;
/// Between cards in a column.
const ROW_GAP: f32 = 14.;
/// Between connected parts, and before the grid of unlinked nodes.
const BLOCK_GAP: f32 = 64.;
/// Columns of the grid of unlinked nodes.
const GRID_COLS: usize = 5;
/// Barycentre sweeps that order each column.
const SWEEPS: usize = 6;
/// Far enough out to fit a long chain of work (a 60-deep one needs 0.06).
pub const MIN_ZOOM: f32 = 0.02;
pub const MAX_ZOOM: f32 = 2.0;
/// Below this zoom cards are dots: their 13 px titles would be under 7 px.
pub const DOTS_BELOW: f32 = 0.55;
/// Room left round everything when fitting.
const FIT_MARGIN: f32 = 48.;
/// What the view's dock covers at the bottom: fitting keeps everything above it.
pub const FIT_BELOW: f32 = 56.;

/// A node as the graph places it. `x`, `y` are its card's top-left corner, in world units.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: Uuid,
    pub key: String,
    pub kind: Kind,
    pub state: State,
    pub title: String,
    pub x: f32,
    pub y: f32,
}

/// A link, drawn from `from` to `to`. For links that order work, `from` comes first: the
/// dependency, what was learned from, what was superseded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub rel: Rel,
}

impl GraphEdge {
    /// Whether the link orders work, and so has an arrow and sets columns.
    pub fn orders(&self) -> bool {
        orders(self.rel)
    }
}

fn orders(rel: Rel) -> bool {
    matches!(rel, Rel::DependsOn | Rel::LearnedFrom | Rel::Supersedes)
}

/// Where the world is on screen: `screen = world * zoom + pan`, in the view's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Camera {
        Camera {
            pan_x: 0.,
            pan_y: 0.,
            zoom: 1.,
        }
    }
}

impl Camera {
    pub fn to_screen(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.zoom + self.pan_x, y * self.zoom + self.pan_y)
    }

    pub fn to_world(&self, x: f32, y: f32) -> (f32, f32) {
        ((x - self.pan_x) / self.zoom, (y - self.pan_y) / self.zoom)
    }

    /// Zoom by `factor`, keeping the world point under screen point (`x`, `y`) where it is.
    pub fn zoom_at(&mut self, factor: f32, x: f32, y: f32) {
        let (wx, wy) = self.to_world(x, y);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan_x = x - wx * self.zoom;
        self.pan_y = y - wy * self.zoom;
    }
}

/// The graph as shown.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub camera: Camera,
    pub selected: Option<usize>,
    pub hovered: Option<usize>,
    /// Where the grid of unlinked nodes starts, for its heading.
    pub unlinked_top: Option<f32>,
    /// Nodes in reading order: block by block, column by column, top down; then the grid.
    reading: Vec<usize>,
    /// The view's size, as last drawn.
    viewport: (f32, f32),
    /// Fit everything once the view's size is known.
    fit_pending: bool,
}

impl Graph {
    /// Lay out `nodes` and the `links` between them. Links to nodes not shown are dropped.
    pub fn build(nodes: Vec<Summary>, links: &[(Uuid, Rel, Uuid)]) -> Graph {
        let mut nodes: Vec<GraphNode> = nodes
            .into_iter()
            .map(|s| GraphNode {
                id: s.id,
                key: s.key,
                kind: s.kind,
                state: s.state,
                title: s.title,
                x: 0.,
                y: 0.,
            })
            .collect();
        // A fixed order in, so the same graph always comes out the same.
        nodes.sort_by_key(sort_key);
        let index: HashMap<Uuid, usize> =
            nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let mut seen = HashSet::new();
        let mut edges = Vec::new();
        for &(src, rel, dst) in links {
            let (Some(&s), Some(&d)) = (index.get(&src), index.get(&dst)) else {
                continue;
            };
            if s == d {
                continue;
            }
            // "src depends on dst": dst comes first, and the arrow points at src.
            let (from, to) = if orders(rel) { (d, s) } else { (s, d) };
            if seen.insert((from, to, rel)) {
                edges.push(GraphEdge { from, to, rel });
            }
        }
        let (reading, unlinked_top) = layout(&mut nodes, &edges);
        Graph {
            nodes,
            edges,
            reading,
            unlinked_top,
            fit_pending: true,
            ..Graph::default()
        }
    }

    /// The node whose card is under screen point (`x`, `y`).
    pub fn node_at(&self, x: f32, y: f32) -> Option<usize> {
        let (wx, wy) = self.camera.to_world(x, y);
        self.nodes
            .iter()
            .position(|n| wx >= n.x && wx <= n.x + CARD_W && wy >= n.y && wy <= n.y + CARD_H)
    }

    /// The node the graph brings out, dimming what is not linked to it: the selected node
    /// while its panel is `open`. A selection with the panel closed is only outlined, and the
    /// pointer brings nothing out: cards have no hover effects.
    pub fn focus(&self, open: bool) -> Option<usize> {
        self.selected.filter(|_| open)
    }

    /// Whether node `i` is drawn at full strength with `focus` brought out.
    pub fn lit(&self, i: usize, focus: Option<usize>) -> bool {
        focus.is_none_or(|f| f == i || self.neighbours(f).contains(&i))
    }

    /// Nodes linked to node `i`, either way.
    pub fn neighbours(&self, i: usize) -> HashSet<usize> {
        self.edges
            .iter()
            .filter_map(|e| {
                if e.from == i {
                    Some(e.to)
                } else if e.to == i {
                    Some(e.from)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Everything in view, with a margin, at a zoom no closer than 1.
    pub fn fit(&mut self) {
        let (vw, vh) = self.viewport;
        if self.nodes.is_empty() || vw <= 0. || vh <= 0. {
            self.camera = Camera::default();
            return;
        }
        let (x0, y0, x1, y1) = self.bounds();
        let (bw, bh) = (x1 - x0, y1 - y0);
        // The area above the dock floating over the bottom.
        let above = (vh - FIT_BELOW).max(1.);
        let zoom = ((vw - 2. * FIT_MARGIN) / bw)
            .min((above - 2. * FIT_MARGIN) / bh)
            .clamp(MIN_ZOOM, 1.);
        self.camera = Camera {
            zoom,
            pan_x: vw / 2. - (x0 + bw / 2.) * zoom,
            pan_y: above / 2. - (y0 + bh / 2.) * zoom,
        };
    }

    /// The world rectangle every card lies in: (left, top, right, bottom).
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        self.nodes.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(x0, y0, x1, y1), n| {
                (
                    x0.min(n.x),
                    y0.min(n.y),
                    x1.max(n.x + CARD_W),
                    y1.max(n.y + CARD_H),
                )
            },
        )
    }

    /// The view's size this frame; fits on the first frame that knows it. When the size
    /// changes (the detail panel opening beside it, say), the selected node stays in view.
    pub fn set_viewport(&mut self, w: f32, h: f32) {
        let resized = self.viewport != (w, h);
        self.viewport = (w, h);
        if self.fit_pending && w > 0. && h > 0. {
            self.fit();
            self.fit_pending = false;
        } else if resized && let Some(i) = self.selected {
            self.reveal(i);
        }
    }

    pub fn viewport(&self) -> (f32, f32) {
        self.viewport
    }

    /// Pan so node `i` is in view, if it is not.
    fn reveal(&mut self, i: usize) {
        let Some(n) = self.nodes.get(i) else { return };
        let (vw, vh) = self.viewport;
        let (sx, sy) = self.camera.to_screen(n.x, n.y);
        let (w, h) = (CARD_W * self.camera.zoom, CARD_H * self.camera.zoom);
        if sx < 0. || sy < 0. || sx + w > vw || sy + h > vh {
            self.camera.pan_x += vw / 2. - (sx + w / 2.);
            self.camera.pan_y += vh / 2. - (sy + h / 2.);
        }
    }

    /// Where a link's line starts and ends, in world units, and whether it runs across (else
    /// down): between columns, from the side of one card facing the other at mid-height;
    /// within a column, from the bottom of the upper card to the top of the lower.
    pub fn ends(&self, e: &GraphEdge) -> ((f32, f32), (f32, f32), bool) {
        let (a, b) = (&self.nodes[e.from], &self.nodes[e.to]);
        let mid = CARD_H / 2.;
        if a.x + CARD_W <= b.x {
            ((a.x + CARD_W, a.y + mid), (b.x, b.y + mid), true)
        } else if b.x + CARD_W <= a.x {
            ((a.x, a.y + mid), (b.x + CARD_W, b.y + mid), true)
        } else if a.y <= b.y {
            (
                (a.x + CARD_W / 2., a.y + CARD_H),
                (b.x + CARD_W / 2., b.y),
                false,
            )
        } else {
            (
                (a.x + CARD_W / 2., a.y),
                (b.x + CARD_W / 2., b.y + CARD_H),
                false,
            )
        }
    }

    /// The reading order, for stepping through nodes.
    pub fn reading(&self) -> &[usize] {
        &self.reading
    }
}

/// A link's line on screen as points along its curve: a gentle S from `a` to `b` whose
/// tangents follow the way the link runs (across, else down). Long links get more points.
pub fn link_curve(a: (f32, f32), b: (f32, f32), across: bool) -> Vec<(f32, f32)> {
    let (c1, c2) = if across {
        let d = (b.0 - a.0) / 2.;
        ((a.0 + d, a.1), (b.0 - d, b.1))
    } else {
        let d = (b.1 - a.1) / 2.;
        ((a.0, a.1 + d), (b.0, b.1 - d))
    };
    let length = (b.0 - a.0).hypot(b.1 - a.1);
    let steps = ((length / 24.).ceil() as usize).clamp(4, 400);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let u = 1. - t;
            let (w0, w1, w2, w3) = (u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t);
            (
                w0 * a.0 + w1 * c1.0 + w2 * c2.0 + w3 * b.0,
                w0 * a.1 + w1 * c1.1 + w2 * c2.1 + w3 * b.1,
            )
        })
        .collect()
}

/// The stretches of a polyline that cross the rectangle (0, 0)–(`w`, `h`) grown by `margin`:
/// only those are drawn, so a long link costs only what shows of it.
pub fn visible_runs(points: &[(f32, f32)], w: f32, h: f32, margin: f32) -> Vec<Vec<(f32, f32)>> {
    let (x0, y0, x1, y1) = (-margin, -margin, w + margin, h + margin);
    let crosses = |p: (f32, f32), q: (f32, f32)| {
        p.0.max(q.0) >= x0 && p.0.min(q.0) <= x1 && p.1.max(q.1) >= y0 && p.1.min(q.1) <= y1
    };
    let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut open = false;
    for pair in points.windows(2) {
        if crosses(pair[0], pair[1]) {
            if !open {
                runs.push(vec![pair[0]]);
                open = true;
            }
            if let Some(run) = runs.last_mut() {
                run.push(pair[1]);
            }
        } else {
            open = false;
        }
    }
    runs
}

/// Kinds in a fixed order, then keys by number.
fn sort_key(n: &GraphNode) -> (u8, u64, String) {
    let kind = match n.kind {
        Kind::Task => 0,
        Kind::Decision => 1,
        Kind::Lesson => 2,
        Kind::Finding => 3,
    };
    let number = n
        .key
        .rsplit('-')
        .next()
        .and_then(|d| d.parse().ok())
        .unwrap_or(u64::MAX);
    (kind, number, n.key.clone())
}

/// Place every node; returns the reading order and where the unlinked grid starts.
fn layout(nodes: &mut [GraphNode], edges: &[GraphEdge]) -> (Vec<usize>, Option<f32>) {
    let n = nodes.len();
    let mut adjacent: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        adjacent[e.from].push(e.to);
        adjacent[e.to].push(e.from);
    }
    // Connected parts, in node order, the largest first.
    let mut part = vec![usize::MAX; n];
    let mut parts: Vec<Vec<usize>> = Vec::new();
    for start in 0..n {
        if part[start] != usize::MAX || adjacent[start].is_empty() {
            continue;
        }
        let id = parts.len();
        let mut members = vec![start];
        part[start] = id;
        let mut i = 0;
        while i < members.len() {
            for &m in &adjacent[members[i]] {
                if part[m] == usize::MAX {
                    part[m] = id;
                    members.push(m);
                }
            }
            i += 1;
        }
        members.sort_unstable();
        parts.push(members);
    }
    parts.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));

    let mut reading = Vec::with_capacity(n);
    let mut top = 0.0f32;
    for members in &parts {
        top += place_part(nodes, edges, members, top, &mut reading) + BLOCK_GAP;
    }

    let unlinked: Vec<usize> = (0..n).filter(|&i| adjacent[i].is_empty()).collect();
    let unlinked_top = (!unlinked.is_empty()).then_some(top);
    for (k, &i) in unlinked.iter().enumerate() {
        nodes[i].x = (k % GRID_COLS) as f32 * (CARD_W + ROW_GAP);
        nodes[i].y = top + (k / GRID_COLS) as f32 * (CARD_H + ROW_GAP);
        reading.push(i);
    }
    (reading, unlinked_top)
}

/// Lay out one connected part from `top`; returns its height.
fn place_part(
    nodes: &mut [GraphNode],
    edges: &[GraphEdge],
    members: &[usize],
    top: f32,
    reading: &mut Vec<usize>,
) -> f32 {
    let inside: HashSet<usize> = members.iter().copied().collect();
    let local: Vec<&GraphEdge> = edges.iter().filter(|e| inside.contains(&e.from)).collect();
    // Columns: one past everything a node comes after. A cycle, which the core refuses for
    // dependencies, would only stop the relaxing after as many passes as there are nodes.
    let mut layer: HashMap<usize, usize> = members.iter().map(|&m| (m, 0)).collect();
    for _ in 0..members.len() {
        let mut changed = false;
        for e in local.iter().filter(|e| e.orders()) {
            let want = layer[&e.from] + 1;
            if layer[&e.to] < want {
                layer.insert(e.to, want);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let depth = layer.values().copied().max().unwrap_or(0) + 1;
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for &m in members {
        columns[layer[&m]].push(m);
    }

    // Order each column by the average place of its neighbours in the column before (sweeping
    // right), then after (sweeping left): the usual way to cut crossings.
    let mut place: HashMap<usize, f32> = HashMap::new();
    let index_places = |columns: &Vec<Vec<usize>>, place: &mut HashMap<usize, f32>| {
        for col in columns {
            for (i, &m) in col.iter().enumerate() {
                place.insert(m, i as f32);
            }
        }
    };
    index_places(&columns, &mut place);
    let mut neighbours: HashMap<usize, Vec<usize>> = HashMap::new();
    for e in &local {
        neighbours.entry(e.from).or_default().push(e.to);
        neighbours.entry(e.to).or_default().push(e.from);
    }
    for sweep in 0..SWEEPS {
        let order: Vec<usize> = if sweep % 2 == 0 {
            (1..depth).collect()
        } else {
            (0..depth.saturating_sub(1)).rev().collect()
        };
        for c in order {
            let towards = if sweep % 2 == 0 { c - 1 } else { c + 1 };
            let mut keyed: Vec<(f32, usize)> = columns[c]
                .iter()
                .map(|&m| {
                    let near: Vec<f32> = neighbours
                        .get(&m)
                        .into_iter()
                        .flatten()
                        .filter(|o| layer[o] == towards)
                        .map(|o| place[o])
                        .collect();
                    let key = if near.is_empty() {
                        place[&m]
                    } else {
                        near.iter().sum::<f32>() / near.len() as f32
                    };
                    (key, m)
                })
                .collect();
            // Ties keep the current order, so a settled column stays put.
            keyed.sort_by(|a, b| {
                a.0.total_cmp(&b.0)
                    .then(place[&a.1].total_cmp(&place[&b.1]))
            });
            columns[c] = keyed.into_iter().map(|(_, m)| m).collect();
            for (i, &m) in columns[c].iter().enumerate() {
                place.insert(m, i as f32);
            }
        }
    }

    let tallest = columns.iter().map(Vec::len).max().unwrap_or(0);
    let pitch = CARD_H + ROW_GAP;
    for (c, col) in columns.iter().enumerate() {
        // Each column centred on the tallest, so short chains sit level with their neighbours.
        let offset = (tallest - col.len()) as f32 * pitch / 2.;
        for (i, &m) in col.iter().enumerate() {
            nodes[m].x = c as f32 * (CARD_W + COL_GAP);
            nodes[m].y = top + offset + i as f32 * pitch;
            reading.push(m);
        }
    }
    tallest as f32 * pitch - ROW_GAP
}

impl Model {
    /// Opening the graph: lay it out, and fit it once its size is known.
    pub(super) fn enter_graph(&mut self) {
        self.graph = Graph::default();
    }

    /// Rebuild the graph from the store, keeping the camera and the selection.
    pub(super) fn load_graph(&mut self) {
        let Some(p) = self.projects.get(self.current) else {
            return;
        };
        let mut all = Vec::new();
        let mut failed = None;
        let show_dismissed = self.display().show_dismissed;
        for kind in [Kind::Task, Kind::Decision, Kind::Lesson, Kind::Finding] {
            let states: Vec<State> = if show_dismissed {
                Vec::new()
            } else {
                kind.states()
                    .iter()
                    .copied()
                    .filter(|s| !s.is_dismissed())
                    .collect()
            };
            match p.ws.list(kind, &states) {
                Ok(rows) => all.extend(rows),
                Err(e) => failed = Some(e.to_string()),
            }
        }
        let links = p.ws.edges().unwrap_or_else(|e| {
            failed = Some(e.to_string());
            Vec::new()
        });
        if let Some(text) = failed {
            self.warn(text);
        }
        let old = std::mem::take(&mut self.graph);
        let mut graph = Graph::build(all, &links);
        let selected = old.selected.and_then(|i| old.nodes.get(i)).map(|n| n.id);
        graph.selected = selected.and_then(|id| graph.nodes.iter().position(|n| n.id == id));
        if !old.nodes.is_empty() {
            graph.camera = old.camera;
            graph.fit_pending = old.fit_pending;
        }
        graph.viewport = old.viewport;
        if graph.fit_pending {
            let (w, h) = graph.viewport;
            graph.set_viewport(w, h);
        }
        self.graph = graph;
    }

    /// The node the graph's selection is on.
    pub(super) fn graph_node(&self) -> Option<Uuid> {
        self.graph
            .selected
            .and_then(|i| self.graph.nodes.get(i))
            .map(|n| n.id)
    }

    /// Step the selection through the reading order, keeping it in view.
    pub(super) fn graph_select_by(&mut self, delta: isize) {
        let reading = self.graph.reading();
        if reading.is_empty() {
            return;
        }
        let at = self
            .graph
            .selected
            .and_then(|s| reading.iter().position(|&r| r == s));
        let next = match at {
            None => 0,
            Some(i) => i.saturating_add_signed(delta).min(reading.len() - 1),
        };
        self.graph_select(reading[next]);
    }

    /// Select node `i`, keep it in view, and follow with the panel if it is open.
    pub fn graph_select(&mut self, i: usize) {
        if i < self.graph.nodes.len() {
            self.graph.selected = Some(i);
            self.graph.reveal(i);
            self.follow_detail();
        }
    }

    /// A click at screen point (`x`, `y`): select the node there and open it. Returns whether
    /// it hit one.
    pub fn graph_click(&mut self, x: f32, y: f32) -> bool {
        match self.graph.node_at(x, y) {
            Some(i) => {
                self.graph.selected = Some(i);
                self.open_detail();
                true
            }
            None => false,
        }
    }

    /// The pointer moved to (`x`, `y`). Returns whether the hovered node changed.
    pub fn graph_hover(&mut self, x: f32, y: f32) -> bool {
        let now = self.graph.node_at(x, y);
        let changed = now != self.graph.hovered;
        self.graph.hovered = now;
        changed
    }

    pub fn graph_pan(&mut self, dx: f32, dy: f32) {
        self.graph.camera.pan_x += dx;
        self.graph.camera.pan_y += dy;
    }

    /// Zoom by `factor` at screen point `at`, or at the middle of the view.
    pub fn graph_zoom(&mut self, factor: f32, at: Option<(f32, f32)>) {
        let (vw, vh) = self.graph.viewport;
        let (x, y) = at.unwrap_or((vw / 2., vh / 2.));
        self.graph.camera.zoom_at(factor, x, y);
    }

    pub fn graph_fit(&mut self) {
        self.graph.fit();
    }

    /// The view's size this frame.
    pub fn graph_viewport(&mut self, w: f32, h: f32) {
        self.graph.set_viewport(w, h);
    }
}
