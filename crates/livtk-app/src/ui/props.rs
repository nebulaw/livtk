//! A node's properties in its panel (what it is and where it stands; when it was made and changed
//! is the meta line under its title, its sessions are among its links): one ordered list of rows, each knowing when it shows and
//! drawn in one place. A new property is a row here; a prop the panel does not know by name
//! still shows, by its name, so nothing set on a node is ever hidden.

use super::*;
use livtk_core::format::duration;

/// The props shown by their own rows; any other shows as itself.
const KNOWN: [&str; 4] = ["agent", "scope", "owner", "target"];

/// A row of a node's properties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Status,
    Priority,
    Kind,
    Scope,
    Owner,
    Target,
    Agent,
    Project,
    Blocked,
    SubTasks,
    /// A prop the panel has no row of its own for, by its name.
    Other(String),
}

/// The rows `d` shows, in their order: what it is and where it stands, then who and where.
pub(super) fn rows(d: &Detail) -> Vec<Row> {
    let n = &d.node;
    let task = n.kind == Kind::Task;
    let project = d.project.as_ref().map(|(g, _, _)| g);
    let mut out = vec![Row::Status];
    out.push(if task { Row::Priority } else { Row::Kind });
    let always = n.props.get("scope").and_then(|v| v.as_str()) == Some("always");
    out.extend(always.then_some(Row::Scope));
    out.extend(project.and_then(|g| g.owner.as_ref()).map(|_| Row::Owner));
    out.extend(project.and_then(|g| g.target.as_ref()).map(|_| Row::Target));
    if task {
        out.extend([Row::Agent, Row::Project]);
        // Waiting matters only while the task is open, as its row in the list says.
        out.extend((n.blocked && !n.kind.is_closed(n.state)).then_some(Row::Blocked));
        out.extend(d.sub_tasks.map(|_| Row::SubTasks));
    }
    let mut other: Vec<&String> = n
        .props
        .iter()
        .filter(|(k, v)| !KNOWN.contains(&k.as_str()) && !v.is_null())
        .map(|(k, _)| k)
        .collect();
    other.sort();
    out.extend(other.into_iter().map(|k| Row::Other(k.clone())));
    out
}

impl LivtkApp {
    /// The panel's properties: each of `rows(d)`, named and drawn.
    pub(super) fn props(&mut self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let mut out = div()
            .debug_selector(|| "detail-props".into())
            .flex()
            .flex_col()
            .gap_2();
        for row in rows(d) {
            let (name, value) = self.prop(&row, d, cx);
            out = out.child(prop_row(&name, value));
        }
        out
    }

    /// One row: its name, and what it shows.
    fn prop(&mut self, row: &Row, d: &Detail, cx: &mut Context<Self>) -> (String, Div) {
        let n = &d.node;
        let project = d.project.as_ref().map(|(g, _, _)| g);
        let text = |s: String| div().child(s);
        match row {
            Row::Status => {
                let state = self.state_button(Menu::DetailState, n.id, n.kind, n.state, true, cx);
                // How long it has been there, beside it.
                let since = d.entered.as_deref().and_then(since).map(|s| {
                    div()
                        .debug_selector(|| "detail-state-since".into())
                        .text_color(color(TEXT_4))
                        .child(s)
                });
                (
                    "Status".into(),
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(state)
                        .children(since),
                )
            }
            Row::Priority => (
                "Priority".into(),
                self.priority_button(Menu::DetailPriority, n.id, n.priority, true, cx),
            ),
            Row::Kind => ("Kind".into(), text(title_case(n.kind.as_str()))),
            Row::Scope => (
                "Scope".into(),
                icon_tag(Icon::Standing, "Standing rule", DONE),
            ),
            Row::Owner => (
                "Owner".into(),
                text(project.and_then(|g| g.owner.clone()).unwrap_or_default()),
            ),
            Row::Target => (
                "Target".into(),
                text(project.and_then(|g| g.target.clone()).unwrap_or_default()),
            ),
            Row::Agent => ("Agent".into(), self.agent_button(d, cx)),
            Row::Project => ("Project".into(), self.project_button(d, cx)),
            Row::Blocked => {
                let open: Vec<&livtk_core::Brief> = d
                    .links
                    .depends_on
                    .iter()
                    .filter(|b| !matches!(b.state, State::Done | State::Canceled))
                    .collect();
                let mut waits = div()
                    .debug_selector(|| "detail-blocked".into())
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(icon(Icon::Blocked, 14., TEXT_3))
                    .child(div().text_color(color(TEXT_3)).child("Waits for"));
                for b in open {
                    let sel = format!("blocked-by-{}", b.key);
                    let chip = div()
                        .id(SharedString::from(sel.clone()))
                        .debug_selector(move || sel.clone())
                        .px_1()
                        .rounded_md()
                        .hover(|h| h.bg(color(HOVER)))
                        .child(b.key.clone());
                    waits = waits.child(self.goes_to(chip, &b.key, OpenIn::View, cx));
                }
                ("Blocked".into(), waits)
            }
            Row::SubTasks => {
                let (done, all) = d.sub_tasks.unwrap_or_default();
                (
                    "Sub-tasks".into(),
                    text(format!("{done} of {all} done"))
                        .debug_selector(|| "detail-sub-tasks".into()),
                )
            }
            Row::Other(name) => {
                let value = match n.props.get(name) {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(v) => v.to_string(),
                    None => String::new(),
                };
                let sel = format!("detail-prop-{name}");
                (
                    title_case(name),
                    text(value).debug_selector(move || sel.clone()),
                )
            }
        }
    }

    /// The task's project: picked from every open project, or none. A sub-task's is its
    /// parent's, named, read-only, and gone to (its project if any, else the parent).
    fn project_button(&self, d: &Detail, cx: &mut Context<Self>) -> Div {
        let id = d.node.id;
        if let Some(parent) = &d.parent_task {
            let (label, to) = match &d.in_project {
                Some(p) => (
                    format!("{}  {}  (through {})", p.key, p.title, parent.key),
                    p.key.clone(),
                ),
                None => (
                    format!("None (as {} is in none)", parent.key),
                    parent.key.clone(),
                ),
            };
            let el = div()
                .id("detail-project-through")
                .debug_selector(|| "detail-project-through".into())
                .rounded_md()
                .hover(|h| h.bg(color(HOVER)))
                .child(label);
            return div().child(self.goes_to(el, &to, OpenIn::View, cx));
        }
        let current = d
            .in_project
            .as_ref()
            .map(|p| (p.key.clone(), p.title.clone()));
        let label = current
            .as_ref()
            .map_or_else(|| "None".to_owned(), |(k, t)| format!("{k}  {t}"));
        let open = self.menu == Some(Menu::DetailProject);
        let button = div()
            .id("detail-project")
            .debug_selector(|| "detail-project".into())
            .h(px(22.))
            .px(px(4.))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .rounded_lg()
            .when(open, |b| b.bg(color(SELECTED)))
            .when(!open, |b| b.hover(|h| h.bg(color(HOVER))))
            .child(icon(Icon::Project, 14., TEXT_3))
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(Menu::DetailProject, window, cx);
                }),
            );
        div().relative().flex_none().child(button).when(open, |el| {
            let mut panel = menu_panel(280.).debug_selector(|| "set-project-menu".into());
            let choices =
                std::iter::once(None).chain(self.model.project_choices().into_iter().map(Some));
            for choice in choices {
                let key = choice.as_ref().map(|(k, _)| k.clone());
                let row_id = format!("set-project-{}", key.as_deref().unwrap_or("none"));
                let label = choice
                    .as_ref()
                    .map_or_else(|| "None".to_owned(), |(k, t)| format!("{k}  {t}"));
                let on = current.as_ref().map(|(k, _)| k) == key.as_ref();
                panel = panel.child(
                    option_row(SharedString::from(row_id.clone()))
                        .debug_selector(move || row_id.clone())
                        .child(div().flex_1().truncate().child(label))
                        .child(check(on))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.model.set_project_of(id, key.clone());
                            this.close_menu(window, cx);
                        })),
                );
            }
            el.child(self.popover(Menu::DetailProject, false, 26., panel, cx))
        })
    }
}

/// "for 2 days": how long since `at` (a time as the log writes it).
fn since(at: &str) -> Option<String> {
    let t: jiff::Timestamp = at.parse().ok()?;
    let secs = jiff::Timestamp::now().duration_since(t).as_secs();
    Some(format!("for {}", duration(secs)))
}
