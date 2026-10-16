//! The graph without a window: its layout, its camera, and acting from it.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use std::time::{Duration, Instant};

use livtk_app::model::{
    CARD_H, CARD_W, Camera, Graph, MAX_ZOOM, MIN_ZOOM, Model, View, link_curve, visible_runs,
};
use livtk_core::{Actor, Command, Kind, Rel, State, Summary, Workspace};
use uuid::Uuid;

const CLAUDE: Actor = Actor::Claude { agent: None };

fn summary(n: u64, kind: Kind) -> Summary {
    Summary {
        id: Uuid::from_u64_pair(0, n),
        key: format!("LIV-{n}"),
        kind,
        title: format!("task {n}"),
        state: State::Backlog,
        priority: 3,
        updated: String::new(),
        blocked: false,
        standing: false,
    }
}

fn id(n: u64) -> Uuid {
    Uuid::from_u64_pair(0, n)
}

/// `a` depends on `b`.
fn dep(a: u64, b: u64) -> (Uuid, Rel, Uuid) {
    (id(a), Rel::DependsOn, id(b))
}

fn at(g: &Graph, n: u64) -> &livtk_app::model::GraphNode {
    g.nodes.iter().find(|x| x.id == id(n)).unwrap()
}

fn tasks(n: u64) -> Vec<Summary> {
    (1..=n).map(|i| summary(i, Kind::Task)).collect()
}

/// How many pairs of arrows cross between two columns.
fn crossings(g: &Graph) -> usize {
    let mut out = 0;
    for (i, a) in g.edges.iter().enumerate() {
        for b in &g.edges[i + 1..] {
            let (a0, a1, b0, b1) = (
                &g.nodes[a.from],
                &g.nodes[a.to],
                &g.nodes[b.from],
                &g.nodes[b.to],
            );
            if a0.x == b0.x && a1.x == b1.x && (a0.y - b0.y) * (a1.y - b1.y) < 0. {
                out += 1;
            }
        }
    }
    out
}

#[test]
fn work_flows_left_to_right_along_its_dependencies() {
    // 3 depends on 2, which depends on 1; 4 depends on 1.
    let g = Graph::build(tasks(4), &[dep(3, 2), dep(2, 1), dep(4, 1)]);
    assert!(
        at(&g, 1).x < at(&g, 2).x,
        "a dependency sits left of what waits for it"
    );
    assert!(at(&g, 2).x < at(&g, 3).x);
    assert_eq!(
        at(&g, 2).x,
        at(&g, 4).x,
        "both wait only on 1: the same column"
    );
    let e = g.edges.iter().find(|e| g.nodes[e.to].id == id(3)).unwrap();
    assert_eq!(
        g.nodes[e.from].id,
        id(2),
        "the arrow points at the one that waits"
    );
    assert!(e.orders());
}

#[test]
fn no_cards_overlap_and_the_same_graph_lays_out_the_same() {
    let links: Vec<_> = (2..=30).map(|i| dep(i, i / 2)).collect();
    let g = Graph::build(tasks(40), &links);
    for (i, a) in g.nodes.iter().enumerate() {
        for b in &g.nodes[i + 1..] {
            let apart = (a.x - b.x).abs() >= CARD_W || (a.y - b.y).abs() >= CARD_H;
            assert!(apart, "{} and {} overlap", a.key, b.key);
        }
    }
    let again = Graph::build(tasks(40), &links);
    assert_eq!(g.nodes, again.nodes, "deterministic");
    let shuffled: Vec<_> = tasks(40).into_iter().rev().collect();
    let reversed_links: Vec<_> = links.iter().rev().copied().collect();
    assert_eq!(
        Graph::build(shuffled, &reversed_links).nodes,
        g.nodes,
        "whatever order the store returns"
    );
}

#[test]
fn columns_are_ordered_to_cut_crossings() {
    // 3 waits on 2 and 4 on 1: in key order the two arrows would cross.
    let g = Graph::build(tasks(4), &[dep(3, 2), dep(4, 1)]);
    assert_eq!(crossings(&g), 0);
    // A wider case: two interleaved fans.
    let links = [dep(5, 2), dep(6, 1), dep(7, 4), dep(8, 3), dep(5, 4)];
    let g = Graph::build(tasks(8), &links);
    assert!(crossings(&g) <= 1, "{} crossings", crossings(&g));
}

#[test]
fn unlinked_nodes_sit_in_a_grid_below_the_linked_parts() {
    let g = Graph::build(tasks(9), &[dep(2, 1), dep(4, 3)]);
    let top = g.unlinked_top.expect("a grid of unlinked nodes");
    for n in 5..=9 {
        assert!(at(&g, n).y >= top, "LIV-{n} is in the grid");
    }
    for n in 1..=4 {
        assert!(at(&g, n).y + CARD_H <= top, "LIV-{n} is above it");
    }
    assert_eq!(at(&g, 5).y, at(&g, 9).y, "five to a row");
    assert!(
        (at(&g, 1).y - at(&g, 3).y).abs() >= CARD_H,
        "separate parts are separate blocks"
    );
    assert!(Graph::build(tasks(2), &[dep(2, 1)]).unlinked_top.is_none());
}

#[test]
fn links_to_nodes_not_shown_are_dropped_and_other_links_have_no_arrow() {
    let links = [
        dep(2, 1),
        dep(3, 99),
        (id(3), Rel::Relates, id(1)),
        (id(1), Rel::DependsOn, id(1)),
    ];
    let g = Graph::build(tasks(3), &links);
    assert_eq!(g.edges.len(), 2);
    let relates = g.edges.iter().find(|e| e.rel == Rel::Relates).unwrap();
    assert!(!relates.orders());
}

#[test]
fn the_camera_zooms_at_the_pointer_and_fits_everything() {
    let mut c = Camera {
        pan_x: 40.,
        ..Camera::default()
    };
    let (wx, wy) = c.to_world(300., 200.);
    c.zoom_at(1.5, 300., 200.);
    let (sx, sy) = c.to_screen(wx, wy);
    assert!(
        (sx - 300.).abs() < 1e-3 && (sy - 200.).abs() < 1e-3,
        "the point stays put"
    );
    c.zoom_at(1000., 0., 0.);
    assert_eq!(c.zoom, MAX_ZOOM);
    c.zoom_at(1e-6, 0., 0.);
    assert_eq!(c.zoom, MIN_ZOOM);

    let links: Vec<_> = (2..=60).map(|i| dep(i, i - 1)).collect();
    let mut g = Graph::build(tasks(60), &links);
    g.set_viewport(1200., 800.);
    let (x0, y0, x1, y1) = g.bounds();
    let (l, t) = g.camera.to_screen(x0, y0);
    let (r, b) = g.camera.to_screen(x1, y1);
    assert!(
        l >= 0. && t >= 0. && r <= 1200. && b <= 800.,
        "all in view: {l} {t} {r} {b}"
    );
    assert!(g.camera.zoom < 1., "a long chain is zoomed out to fit");

    let mut small = Graph::build(tasks(2), &[dep(2, 1)]);
    small.set_viewport(1200., 800.);
    assert_eq!(small.camera.zoom, 1., "a small graph is never blown up");
}

#[test]
fn a_point_finds_the_card_under_it() {
    let mut g = Graph::build(tasks(2), &[dep(2, 1)]);
    g.set_viewport(1000., 600.);
    let n = at(&g, 2).clone();
    let (sx, sy) = g.camera.to_screen(n.x + 10., n.y + 10.);
    assert_eq!(g.node_at(sx, sy).map(|i| g.nodes[i].id), Some(id(2)));
    let (gx, gy) = g.camera.to_screen(n.x - 20., n.y);
    assert_eq!(
        g.node_at(gx, gy),
        None,
        "the gap between columns is background"
    );
}

fn create(ws: &mut Workspace, kind: Kind, title: &str) -> String {
    let e = ws
        .execute(
            Command::Create {
                kind,
                title: title.into(),
                body: String::new(),
                priority: None,
                links: vec![],
            },
            &CLAUDE,
        )
        .unwrap();
    ws.node(&e[0].op.subjects().0.to_string()).unwrap().key
}

/// A plan waiting on a task, a canceled task, and a lesson learned from the plan.
fn app(dir: &std::path::Path) -> Model {
    let mut ws = Workspace::init(dir, Some("LIV")).unwrap();
    let a = create(&mut ws, Kind::Task, "foundation");
    let b = create(&mut ws, Kind::Task, "the plan");
    let c = create(&mut ws, Kind::Task, "dropped");
    let l = create(&mut ws, Kind::Lesson, "what the plan taught");
    for (src, rel, dst) in [(&b, Rel::DependsOn, &a), (&l, Rel::LearnedFrom, &b)] {
        ws.execute(
            Command::Link {
                src: src.clone(),
                rel,
                dst: dst.clone(),
            },
            &CLAUDE,
        )
        .unwrap();
    }
    for (k, to) in [(&b, State::Planned), (&c, State::Canceled)] {
        ws.execute(
            Command::Move {
                node: k.clone(),
                to,
            },
            &CLAUDE,
        )
        .unwrap();
    }
    drop(ws);
    let mut m = Model::open(&[dir.to_path_buf()]);
    m.set_models_dir(None);
    m
}

fn keys(m: &Model) -> Vec<&str> {
    let mut k: Vec<&str> = m.graph.nodes.iter().map(|n| n.key.as_str()).collect();
    k.sort_unstable();
    k
}

#[test]
fn the_graph_shows_every_kind_and_hides_dismissed_work_until_h() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Graph);
    assert_eq!(keys(&m), ["LES-1", "LIV-1", "LIV-2"]);
    assert_eq!(m.graph.edges.len(), 2);
    assert!(m.rows.is_empty(), "the graph has no rows");
    m.toggle_show_all();
    assert_eq!(keys(&m), ["LES-1", "LIV-1", "LIV-2", "LIV-3"]);
    let lesson = m.graph.nodes.iter().find(|n| n.key == "LES-1").unwrap();
    let plan = m.graph.nodes.iter().find(|n| n.key == "LIV-2").unwrap();
    assert!(
        plan.x < lesson.x,
        "a lesson sits right of the work it was learned from"
    );
}

#[test]
fn the_view_fits_once_its_size_is_known_and_keeps_the_camera_after() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Graph);
    m.graph_viewport(900., 600.);
    let fitted = m.graph.camera;
    m.graph_pan(30., -10.);
    m.graph_viewport(900., 600.);
    assert_eq!(
        m.graph.camera.pan_x,
        fitted.pan_x + 30.,
        "fits once, not every frame"
    );

    let mut ws = Workspace::open(dir.path()).unwrap();
    create(&mut ws, Kind::Task, "arrives while looking");
    assert!(m.tick());
    assert_eq!(m.graph.nodes.len(), 4);
    assert_eq!(
        m.graph.camera.pan_x,
        fitted.pan_x + 30.,
        "a change keeps the camera"
    );

    m.graph_zoom(2., None);
    assert!((m.graph.camera.zoom - (fitted.zoom * 2.).min(MAX_ZOOM)).abs() < 1e-4);
    m.graph_fit();
    let (x0, y0, x1, y1) = m.graph.bounds();
    let (l, t) = m.graph.camera.to_screen(x0, y0);
    let (r, b) = m.graph.camera.to_screen(x1, y1);
    assert!(
        l >= 0. && t >= 0. && r <= 900. && b <= 600.,
        "0 fits again, the new node too"
    );
}

#[test]
fn a_click_opens_a_node_and_its_actions_act_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Graph);
    m.graph_viewport(1200., 700.);
    let plan = m.graph.nodes.iter().position(|n| n.key == "LIV-2").unwrap();
    let n = m.graph.nodes[plan].clone();
    let (x, y) = m.graph.camera.to_screen(n.x + 5., n.y + 5.);
    assert!(m.graph_click(x, y));
    assert_eq!(m.view, View::Graph, "the graph stays");
    assert_eq!(m.detail.as_ref().unwrap().node.key, "LIV-2");
    m.decide();
    let ws = Workspace::open(dir.path()).unwrap();
    assert_eq!(ws.node("LIV-2").unwrap().state, State::Acked);
    assert_eq!(
        m.graph.selected.map(|i| m.graph.nodes[i].key.as_str()),
        Some("LIV-2"),
        "the selection survives the change"
    );
    assert!(!m.graph_click(-500., -500.), "the background opens nothing");
}

#[test]
fn j_and_k_step_through_nodes_in_reading_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Graph);
    m.graph_viewport(1200., 700.);
    let mut seen = Vec::new();
    for _ in 0..3 {
        m.select_by(1);
        let i = m.graph.selected.unwrap();
        seen.push(m.graph.nodes[i].key.clone());
    }
    assert_eq!(
        seen,
        ["LIV-1", "LIV-2", "LES-1"],
        "left to right along the work"
    );
    m.select_by(1);
    assert_eq!(
        m.graph.nodes[m.graph.selected.unwrap()].key,
        "LES-1",
        "stops at the end"
    );
    m.open_detail();
    m.select_by(-1);
    assert_eq!(
        m.detail.as_ref().unwrap().node.key,
        "LIV-2",
        "the panel follows"
    );
}

#[test]
fn hovering_tracks_the_card_under_the_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.set_view(View::Graph);
    m.graph_viewport(1200., 700.);
    let n = m.graph.nodes[0].clone();
    let (x, y) = m.graph.camera.to_screen(n.x + 1., n.y + 1.);
    assert!(m.graph_hover(x, y));
    assert_eq!(m.graph.hovered, Some(0));
    assert!(!m.graph_hover(x + 1., y), "same card, nothing to redraw");
    assert!(m.graph_hover(-100., -100.));
    assert_eq!(m.graph.hovered, None);
}

/// A fixed pseudo-random graph: each node waits on up to two earlier ones.
fn big(n: u64) -> (Vec<Summary>, Vec<(Uuid, Rel, Uuid)>) {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut links = Vec::new();
    for i in 2..=n {
        for _ in 0..(next() % 3) {
            links.push(dep(i, 1 + next() % (i - 1)));
        }
    }
    (tasks(n), links)
}

/// `cargo test --release -p livtk-app --test graph -- --ignored --nocapture`
#[test]
#[ignore = "a benchmark: run in release with --ignored"]
fn layout_stays_in_budget() {
    for (n, budget) in [(100, 10), (1_000, 150), (10_000, 1_000)] {
        let (nodes, links) = big(n);
        let best = (0..5)
            .map(|_| {
                let t = Instant::now();
                Graph::build(nodes.clone(), &links);
                t.elapsed()
            })
            .min()
            .unwrap();
        eprintln!("layout of {n} nodes, {} links: {best:?}", links.len());
        assert!(best < Duration::from_millis(budget), "budget {budget} ms");
    }
}

#[test]
fn the_palette_goes_to_the_graph_and_fits_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = app(dir.path());
    m.open_palette();
    m.palette_query("go to graph");
    m.palette_run();
    assert_eq!(m.view, View::Graph);
    m.graph_viewport(900., 600.);
    let fitted = m.graph.camera;
    m.graph_pan(400., 400.);
    m.open_palette();
    m.palette_query("zoom to fit");
    m.palette_run();
    assert_eq!(m.graph.camera, fitted);
}

#[test]
fn links_run_between_the_facing_sides_of_their_cards() {
    let g = Graph::build(tasks(3), &[dep(2, 1), (id(3), Rel::Relates, id(1))]);
    let across = g.edges.iter().find(|e| e.orders()).unwrap();
    let ((x0, y0), (x1, y1), horizontal) = g.ends(across);
    let (a, b) = (&g.nodes[across.from], &g.nodes[across.to]);
    assert!(horizontal);
    assert_eq!(
        (x0, y0),
        (a.x + CARD_W, a.y + CARD_H / 2.),
        "leaves the right side"
    );
    assert_eq!(
        (x1, y1),
        (b.x, b.y + CARD_H / 2.),
        "arrives at the left side"
    );
    let down = g.edges.iter().find(|e| !e.orders()).unwrap();
    let (_, _, horizontal) = g.ends(down);
    assert!(
        !horizontal,
        "3 relates to 1, both first: one column, joined top to bottom"
    );
}

#[test]
fn a_narrower_view_keeps_the_selected_node_in_it() {
    let links: Vec<_> = (2..=6).map(|i| dep(i, i - 1)).collect();
    let mut g = Graph::build(tasks(6), &links);
    g.set_viewport(2000., 600.);
    let last = g.nodes.iter().position(|n| n.id == id(6)).unwrap();
    g.selected = Some(last);
    // The detail panel opens: 480 px fewer.
    g.set_viewport(1520., 600.);
    let n = &g.nodes[last];
    let (x, _) = g.camera.to_screen(n.x, n.y);
    assert!(
        x >= 0. && x + CARD_W * g.camera.zoom <= 1520.,
        "still in view at {x}"
    );
}

#[test]
fn a_link_curve_runs_end_to_end_with_level_tangents() {
    let pts = link_curve((0., 0.), (300., 100.), true);
    assert_eq!(pts.first(), Some(&(0., 0.)));
    let last = *pts.last().unwrap();
    assert!((last.0 - 300.).abs() < 1e-3 && (last.1 - 100.).abs() < 1e-3);
    let slope = (pts[1].1 - pts[0].1) / (pts[1].0 - pts[0].0);
    assert!(slope.abs() < 0.1, "leaves level: slope {slope}");
    assert!(
        pts.windows(2).all(|w| w[1].0 >= w[0].0),
        "never doubles back"
    );
    assert!(
        link_curve((0., 0.), (5000., 0.), true).len() > 100,
        "long links get more points"
    );
    assert_eq!(
        link_curve((0., 0.), (1., 1.), true).len(),
        5,
        "short ones a few"
    );
}

#[test]
fn only_the_stretches_in_view_are_kept() {
    // A line from far left to far right through the view, and back out above it.
    let pts = [
        (-1000., 50.),
        (-500., 50.),
        (100., 50.),
        (700., 50.),
        (1500., 50.),
        (1500., -900.),
    ];
    let runs = visible_runs(&pts, 800., 600., 10.);
    assert_eq!(
        runs,
        [vec![(-500., 50.), (100., 50.), (700., 50.), (1500., 50.)]]
    );
    let away = [(-1000., -1000.), (-900., -900.)];
    assert!(visible_runs(&away, 800., 600., 10.).is_empty());
    let twice = [
        (-50., 10.),
        (50., 10.),
        (50., -500.),
        (60., -500.),
        (60., 10.),
    ];
    assert_eq!(
        visible_runs(&twice, 800., 600., 0.).len(),
        2,
        "out and back in again"
    );
}

#[test]
fn the_open_or_hovered_node_is_brought_out_and_the_rest_dimmed() {
    let mut g = Graph::build(tasks(3), &[dep(2, 1)]);
    let (one, two, three) = (
        g.nodes.iter().position(|n| n.id == id(1)).unwrap(),
        g.nodes.iter().position(|n| n.id == id(2)).unwrap(),
        g.nodes.iter().position(|n| n.id == id(3)).unwrap(),
    );
    assert_eq!(g.focus(false), None);
    assert!(
        (0..3).all(|i| g.lit(i, None)),
        "nothing in focus, nothing dimmed"
    );

    g.selected = Some(two);
    assert_eq!(
        g.focus(false),
        None,
        "selected with the panel closed: only outlined"
    );
    let f = g.focus(true);
    assert_eq!(f, Some(two), "open in the panel: brought out");
    assert!(g.lit(two, f) && g.lit(one, f), "it and what it links to");
    assert!(!g.lit(three, f), "the rest dims");

    g.hovered = Some(three);
    assert_eq!(
        g.focus(true),
        Some(three),
        "the pointer wins while it is on a card"
    );
}
