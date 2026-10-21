//! The SQLite store: every event, plus the current state built from them.
//!
//! The store can always be rebuilt from the log, so a schema change is a version bump and a
//! rebuild, never a data migration. Applying an event never fails on content: an event that
//! cannot apply (a set on a node that does not exist, say) is kept in `events` and reported as a
//! warning, so replay of any merged log always completes.

use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::event::{Event, NodeData, NodePatch, Op};
use crate::log::Mark;
use crate::model::{Key, Kind, Rel, State};
use crate::{Error, Result};

const SCHEMA_VERSION: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE events (
    id    TEXT PRIMARY KEY,
    at    TEXT NOT NULL,
    by    TEXT NOT NULL,
    op    TEXT NOT NULL,
    node  TEXT NOT NULL,
    other TEXT,
    line  TEXT NOT NULL
);
CREATE INDEX events_node ON events (node, id);
CREATE INDEX events_other ON events (other, id) WHERE other IS NOT NULL;

CREATE TABLE nodes (
    id         TEXT PRIMARY KEY,
    key        TEXT NOT NULL UNIQUE,
    kind       TEXT NOT NULL,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL,
    state      TEXT NOT NULL,
    priority   INTEGER NOT NULL,
    props      TEXT NOT NULL,
    created    TEXT NOT NULL,
    updated    TEXT NOT NULL,
    created_by TEXT NOT NULL
);
CREATE INDEX nodes_kind_state ON nodes (kind, state);

CREATE TABLE edges (
    src TEXT NOT NULL,
    rel TEXT NOT NULL,
    dst TEXT NOT NULL,
    PRIMARY KEY (src, rel, dst)
) WITHOUT ROWID;
CREATE INDEX edges_dst ON edges (dst, rel);

-- Nodes linked to docs: files in the project, by their path from its folder.
CREATE TABLE doc_links (
    node TEXT NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (node, path)
) WITHOUT ROWID;
CREATE INDEX doc_links_path ON doc_links (path);

CREATE TABLE counters (
    prefix TEXT PRIMARY KEY,
    last   INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE meta (
    k TEXT PRIMARY KEY,
    v TEXT NOT NULL
) WITHOUT ROWID;

-- Keyword search over nodes. External content: the text lives in `nodes`; the index is kept in
-- step by the code that applies events, never by triggers, so a rebuild can clear it at once.
CREATE VIRTUAL TABLE nodes_fts USING fts5 (
    title, body, content = 'nodes', content_rowid = 'rowid', tokenize = 'porter unicode61'
);

-- Embeddings, keyed by node and a hash of the text and model they came from. Derived data that
-- a rebuild keeps, so rebuilding never re-embeds.
CREATE TABLE vectors (
    node  TEXT PRIMARY KEY,
    hash  TEXT NOT NULL,
    vec   BLOB NOT NULL
) WITHOUT ROWID;
";

/// The memory recall draws from: ratified lessons that are not already standing rules, accepted
/// decisions, and findings.
const RECALLABLE: &str = "((n.kind = 'lesson' AND n.state = 'ratified' \
    AND coalesce(json_extract(n.props, '$.scope'), '') <> 'always') \
    OR (n.kind = 'decision' AND n.state = 'accepted') OR n.kind = 'finding')";

/// True when node `n` waits on a dependency that is neither done nor canceled.
macro_rules! blocked {
    () => {
        "EXISTS (SELECT 1 FROM edges e JOIN nodes d ON d.id = e.dst \
         WHERE e.src = n.id AND e.rel = 'depends_on' AND d.state NOT IN ('done', 'canceled'))"
    };
}
const BLOCKED: &str = blocked!();

/// What a [`Summary`] is read from, in `summary_from`'s order; `BLOCKED` follows `updated`.
const SUMMARY_COLS: &str = concat!(
    "n.id, n.key, n.kind, n.title, n.state, n.priority, n.updated, ",
    blocked!(),
    ", coalesce(json_extract(n.props, '$.scope'), '') = 'always', \
     (SELECT e.dst FROM edges e WHERE e.src = n.id AND e.rel = 'parent' LIMIT 1)"
);

const NODE_COLS: &str = "n.id, n.key, n.kind, n.title, n.body, n.state, n.priority, n.props, \
    n.created, n.updated, n.created_by";

/// How far the store has applied the log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub mark: Mark,
    pub max_id: Option<Uuid>,
}

/// A node as the projection holds it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Node {
    pub id: Uuid,
    pub key: String,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub state: State,
    pub priority: u8,
    pub props: Map<String, Value>,
    pub created: String,
    pub updated: String,
    pub created_by: String,
    /// Waits on a dependency that is neither done nor canceled.
    pub blocked: bool,
}

/// A node as a list shows it: no body, no props. Lists can hold thousands of these, so they stay
/// small; open a single node for the rest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub id: Uuid,
    pub key: String,
    pub kind: Kind,
    pub title: String,
    pub state: State,
    pub priority: u8,
    pub updated: String,
    pub blocked: bool,
    /// A ratified lesson marked as a standing rule.
    pub standing: bool,
    /// The task this one is a sub-task of.
    pub parent: Option<Uuid>,
}

/// Events of one kind on one day: see `Store::events_by_day`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DayEvents {
    /// The day, `YYYY-MM-DD`, in this machine's time zone.
    pub day: String,
    /// Made by the human, else by Claude (or a hook).
    pub human: bool,
    /// The operation, as `node.set`.
    pub op: String,
    /// For a `node.set` that moved the node, the state it moved to.
    pub state: Option<String>,
    pub count: u64,
}

/// A short reference to a node, for lists of links.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Brief {
    pub key: String,
    pub title: String,
    pub state: State,
    /// Its kind, which says whether its state is closed. Not in the JSON, which stays as it was.
    #[serde(skip)]
    pub kind: Kind,
}

/// The full projected state, in a fixed order. Two stores built from the same events give equal
/// snapshots.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Snapshot {
    pub nodes: Vec<Node>,
    pub edges: Vec<(Uuid, Rel, Uuid)>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Store { conn })
    }

    pub fn has_current_schema(&self) -> Result<bool> {
        let v: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        Ok(v == SCHEMA_VERSION)
    }

    /// Drop whatever is there and create the current schema, empty. The caller holds the
    /// workspace lock and rebuilds from the log afterwards.
    pub fn reset_schema(&mut self) -> Result<()> {
        self.conn.pragma_update(None, "journal_mode", "WAL")?;
        let tx = self.conn.transaction()?;
        let tables: Vec<String> = tx
            .prepare(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for t in tables {
            tx.execute_batch(&format!(
                "DROP TABLE IF EXISTS \"{}\";",
                t.replace('"', "\"\"")
            ))?;
        }
        tx.execute_batch(SCHEMA)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        Ok(())
    }

    pub fn applied(&self) -> Result<Applied> {
        let get = |k: &str| -> Result<Option<String>> {
            Ok(self
                .conn
                .query_row("SELECT v FROM meta WHERE k = ?1", [k], |r| r.get(0))
                .optional()?)
        };
        let num =
            |k: &str| -> Result<u64> { Ok(get(k)?.and_then(|v| v.parse().ok()).unwrap_or(0)) };
        Ok(Applied {
            mark: Mark {
                len: num("log_len")?,
                head: num("log_head")?,
                tail: num("log_tail")?,
            },
            max_id: get("max_id")?.and_then(|v| v.parse().ok()),
        })
    }

    /// Apply events in order, in one transaction, and record how far the log is applied. With
    /// `reset`, the projection is emptied first (a rebuild). Returns warnings for events that
    /// could not apply.
    pub fn apply(
        &mut self,
        events: &[Event],
        applied: Applied,
        reset: bool,
    ) -> Result<Vec<String>> {
        let tx = self.conn.transaction()?;
        if reset {
            tx.execute_batch(
                "INSERT INTO nodes_fts (nodes_fts) VALUES ('delete-all'); \
                 DELETE FROM events; DELETE FROM nodes; DELETE FROM edges; \
                 DELETE FROM doc_links; DELETE FROM counters;",
            )?;
        }
        let mut warnings = Vec::new();
        for e in events {
            if let Some(w) = apply_one(&tx, e)? {
                warnings.push(w);
            }
        }
        let meta = [
            ("log_len", applied.mark.len.to_string()),
            ("log_head", applied.mark.head.to_string()),
            ("log_tail", applied.mark.tail.to_string()),
            (
                "max_id",
                applied.max_id.map(|i| i.to_string()).unwrap_or_default(),
            ),
        ];
        for (k, v) in meta {
            tx.execute(
                "INSERT OR REPLACE INTO meta (k, v) VALUES (?1, ?2)",
                params![k, v],
            )?;
        }
        tx.commit()?;
        if reset {
            // A rebuild rewrites every page into the WAL; fold it back so it does not linger.
            self.conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        }
        Ok(warnings)
    }

    // ---- queries ----------------------------------------------------------------------------

    /// Find a node by uuid, key (`LIV-12`, any case), or a bare number under `prefix`.
    pub fn resolve(&self, reference: &str, prefix: &str) -> Result<Option<Uuid>> {
        let r = reference.trim();
        if let Ok(id) = r.parse::<Uuid>() {
            let found: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM nodes WHERE id = ?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            return Ok(found.and(Some(id)));
        }
        let key = if !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()) {
            format!("{prefix}-{r}")
        } else {
            r.to_ascii_uppercase()
        };
        let found: Option<String> = self
            .conn
            .query_row("SELECT id FROM nodes WHERE key = ?1", [&key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(found.and_then(|s| s.parse().ok()))
    }

    pub fn node(&self, id: Uuid) -> Result<Option<Node>> {
        let sql = format!("SELECT {NODE_COLS}, {BLOCKED} FROM nodes n WHERE n.id = ?1");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut rows = stmt.query([id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(node_from(row)?)),
            None => Ok(None),
        }
    }

    /// Nodes of a kind in the given states (all states when empty), work in hand first.
    /// Each kind nodes are of, as stored, with their keys.
    /// Each kind and state nodes are in, as stored: one pass over their index.
    pub fn kind_states(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT DISTINCT kind, state FROM nodes")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// The first `n` keys of nodes of `kind`, in `state` if given.
    pub fn keys_in(&self, kind: &str, state: Option<&str>, n: u32) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT key FROM nodes WHERE kind = ?1 AND (?2 IS NULL OR state = ?2) \
             ORDER BY id LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![kind, state, n], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Every node under `id` through `parent` links, nested ones too: its parts, theirs, and so
    /// on, each once.
    pub fn parts_under(&self, id: Uuid) -> Result<Vec<Summary>> {
        let sql = format!(
            "WITH RECURSIVE under(id) AS ( \
                 SELECT src FROM edges WHERE dst = ?1 AND rel = 'parent' \
                 UNION SELECT e.src FROM edges e JOIN under u ON e.dst = u.id \
                 WHERE e.rel = 'parent') \
             SELECT {SUMMARY_COLS} FROM nodes n JOIN under u ON n.id = u.id ORDER BY n.id"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut rows = stmt.query([id.to_string()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(summary_from(row)?);
        }
        Ok(out)
    }

    /// Each node under a project, with the nearest project it is part of: none when no
    /// project exists, at no cost.
    pub fn projects_of(&self) -> Result<Vec<(Uuid, Uuid, Brief)>> {
        let any: Option<i64> = self
            .conn
            .prepare_cached("SELECT 1 FROM nodes WHERE kind = 'project' LIMIT 1")?
            .query_row([], |r| r.get(0))
            .optional()?;
        if any.is_none() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare_cached(
            "WITH RECURSIVE under(id, project, depth) AS ( \
                 SELECT e.src, e.dst, 1 FROM edges e JOIN nodes p ON p.id = e.dst \
                 WHERE e.rel = 'parent' AND p.kind = 'project' \
                 UNION ALL SELECT e.src, u.project, u.depth + 1 FROM edges e \
                 JOIN under u ON e.dst = u.id WHERE e.rel = 'parent' AND u.depth < 64) \
             SELECT u.id, p.key, p.title, p.state, min(u.depth), p.id FROM under u \
             JOIN nodes p ON p.id = u.project GROUP BY u.id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let state: String = row.get(3)?;
            let project: String = row.get(5)?;
            out.push((
                parse_uuid(&id)?,
                parse_uuid(&project)?,
                Brief {
                    key: row.get(1)?,
                    title: row.get(2)?,
                    state: state.parse()?,
                    kind: Kind::Project,
                },
            ));
        }
        Ok(out)
    }

    /// A node's notes that carry a health, newest first: when, the health, the text.
    pub fn updates(&self, id: Uuid) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT at, json_extract(line, '$.data.health'), json_extract(line, '$.data.text') \
             FROM events WHERE node = ?1 AND op = 'node.note' \
             AND json_extract(line, '$.data.health') IS NOT NULL ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([id.to_string()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Every state change of nodes of `kind`, in the log's order: the node, the state it
    /// entered, and when (its creation counts as entering its first state).
    pub fn state_changes(&self, kind: Kind) -> Result<Vec<(Uuid, State, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT e.node, json_extract(e.line, '$.data.state'), e.at FROM events e \
             JOIN nodes n ON n.id = e.node WHERE n.kind = ?1 \
             AND e.op IN ('node.create', 'node.set') \
             AND json_extract(e.line, '$.data.state') IS NOT NULL ORDER BY e.id",
        )?;
        let mut rows = stmt.query([kind.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let node: String = row.get(0)?;
            let state: String = row.get(1)?;
            out.push((parse_uuid(&node)?, state.parse()?, row.get(2)?));
        }
        Ok(out)
    }

    /// When a node last changed state, or was created if it never has.
    pub fn entered(&self, id: Uuid) -> Result<Option<String>> {
        let at: Option<String> = self
            .conn
            .prepare_cached(
                "SELECT max(at) FROM events WHERE node = ?1 AND (op = 'node.create' \
                 OR (op = 'node.set' AND json_extract(line, '$.data.state') IS NOT NULL))",
            )?
            .query_row([id.to_string()], |r| r.get(0))?;
        Ok(at)
    }

    /// A node's newest note that carries a health, as when, the health, and the text.
    pub fn last_update(&self, id: Uuid) -> Result<Option<(String, String, String)>> {
        Ok(self
            .conn
            .prepare_cached(
                "SELECT at, json_extract(line, '$.data.health'), json_extract(line, '$.data.text') \
                 FROM events WHERE node = ?1 AND op = 'node.note' \
                 AND json_extract(line, '$.data.health') IS NOT NULL ORDER BY id DESC LIMIT 1",
            )?
            .query_row([id.to_string()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?)
    }

    /// The key of the node with this id, if the store has it.
    pub fn key_of(&self, id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT key FROM nodes WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    pub fn list(&self, kind: Kind, states: &[State]) -> Result<Vec<Summary>> {
        let wanted: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM nodes n WHERE n.kind = ?1 \
             AND (?2 = '[]' OR n.state IN (SELECT value FROM json_each(?2)))"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut rows = stmt.query(params![kind.as_str(), serde_json::to_string(&wanted)?])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(summary_from(row)?);
        }
        // Sorting here rather than in SQL keeps SQLite from copying rows through its sorter.
        out.sort_unstable_by(|a, b| {
            (a.state.rank(), a.priority, a.id).cmp(&(b.state.rank(), b.priority, b.id))
        });
        Ok(out)
    }

    /// The best acknowledged task with nothing left to wait for: highest priority first, then
    /// the one that unblocks the most, then the oldest.
    pub fn next(&self) -> Result<Option<Node>> {
        let sql = format!(
            "SELECT {NODE_COLS}, {BLOCKED} FROM nodes n \
             WHERE n.kind = 'task' AND n.state = 'acked' AND NOT {BLOCKED} \
             ORDER BY n.priority, \
               (SELECT count(*) FROM edges e WHERE e.dst = n.id AND e.rel = 'depends_on') DESC, \
               n.id \
             LIMIT 1"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(node_from(row)?)),
            None => Ok(None),
        }
    }

    /// The nodes `id` points to over `rel`, and the nodes pointing to it.
    pub fn links(&self, id: Uuid, rel: Rel) -> Result<(Vec<Brief>, Vec<Brief>)> {
        let out = self.briefs(
            "SELECT d.key, d.title, d.state, d.kind FROM edges e JOIN nodes d ON d.id = e.dst \
             WHERE e.src = ?1 AND e.rel = ?2 ORDER BY d.id",
            id,
            rel,
        )?;
        let inc = self.briefs(
            "SELECT s.key, s.title, s.state, s.kind FROM edges e JOIN nodes s ON s.id = e.src \
             WHERE e.dst = ?1 AND e.rel = ?2 ORDER BY s.id",
            id,
            rel,
        )?;
        Ok((out, inc))
    }

    fn briefs(&self, sql: &str, id: Uuid, rel: Rel) -> Result<Vec<Brief>> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = stmt.query(params![id.to_string(), rel.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let state: String = row.get(2)?;
            let kind: String = row.get(3)?;
            out.push(Brief {
                key: row.get(0)?,
                title: row.get(1)?,
                state: state.parse()?,
                kind: kind.parse()?,
            });
        }
        Ok(out)
    }

    pub fn edge_exists(&self, src: Uuid, rel: Rel, dst: Uuid) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM edges WHERE src = ?1 AND rel = ?2 AND dst = ?3",
                params![src.to_string(), rel.as_str(), dst.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether `to` can be reached from `from` by following `rel` edges forward.
    pub fn reaches(&self, from: Uuid, to: Uuid, rel: Rel) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "WITH RECURSIVE r(id) AS ( \
                   SELECT ?1 \
                   UNION SELECT e.dst FROM edges e JOIN r ON e.src = r.id WHERE e.rel = ?3 \
                 ) SELECT 1 FROM r WHERE id = ?2 LIMIT 1",
                params![from.to_string(), to.to_string(), rel.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// The next free number for keys with `prefix`.
    pub fn next_number(&self, prefix: &str) -> Result<u64> {
        Ok(next_number(&self.conn, prefix)?)
    }

    /// Every event about a node, oldest first.
    pub fn history(&self, id: Uuid) -> Result<Vec<Event>> {
        self.events_where("WHERE node = ?1 OR other = ?1", Some(id))
    }

    /// A page of the log, newest first: up to `limit` events before `before`, or the newest.
    pub fn events_page(&self, before: Option<Uuid>, limit: usize) -> Result<Vec<Event>> {
        // uuidv7s in their text form sort as their times do.
        let mut stmt = self.conn.prepare_cached(
            "SELECT line FROM events WHERE ?1 IS NULL OR id < ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let lines: Vec<String> = stmt
            .query_map(params![before.map(|b| b.to_string()), limit as i64], |r| {
                r.get(0)
            })?
            .collect::<rusqlite::Result<_>>()?;
        lines
            .iter()
            .map(|l| serde_json::from_str(l).map_err(Error::from))
            .collect()
    }

    /// A page of the log between `from` (inclusive) and `to` (exclusive), times as the log
    /// writes them (UTC, RFC 3339), newest first: up to `limit` events before `before`.
    pub fn events_page_in(
        &self,
        from: &str,
        to: &str,
        before: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT line FROM events WHERE at >= ?1 AND at < ?2 AND (?3 IS NULL OR id < ?3) \
             ORDER BY id DESC LIMIT ?4",
        )?;
        let lines: Vec<String> = stmt
            .query_map(
                params![from, to, before.map(|b| b.to_string()), limit as i64],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        lines
            .iter()
            .map(|l| serde_json::from_str(l).map_err(Error::from))
            .collect()
    }

    /// How many events each day holds, in this machine's time zone: by whether the human made
    /// them, the operation, and for a move the state moved to. One grouped pass over the log.
    pub fn events_by_day(&self) -> Result<Vec<DayEvents>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT date(at, 'localtime'), by = 'human', op, \
             json_extract(line, '$.data.state'), count(*) \
             FROM events GROUP BY 1, 2, 3, 4",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(DayEvents {
                    day: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    human: r.get(1)?,
                    op: r.get(2)?,
                    state: r.get(3)?,
                    count: r.get::<_, i64>(4)?.max(0) as u64,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// The docs `node` is linked to, by path.
    pub fn docs_of(&self, node: Uuid) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT path FROM doc_links WHERE node = ?1 ORDER BY path")?;
        let rows = stmt
            .query_map([node.to_string()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// The nodes linked to the doc at `path`.
    pub fn doc_nodes(&self, path: &str) -> Result<Vec<Uuid>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT node FROM doc_links WHERE path = ?1 ORDER BY node")?;
        let rows: Vec<String> = stmt
            .query_map([path], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows.iter().map(|s| parse_uuid(s)).collect()
    }

    /// The nodes whose title, plan or notes mention `text`, newest change first.
    pub fn mentioning(&self, text: &str) -> Result<Vec<Uuid>> {
        let like = format!(
            "%{}%",
            text.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self.conn.prepare_cached(
            "SELECT id FROM nodes n WHERE n.title LIKE ?1 ESCAPE '\\' OR n.body LIKE ?1 ESCAPE '\\' \
             OR EXISTS (SELECT 1 FROM events e WHERE e.node = n.id AND e.op = 'node.note' \
                 AND e.line LIKE ?1 ESCAPE '\\') \
             ORDER BY n.updated DESC",
        )?;
        let rows: Vec<String> = stmt
            .query_map([like], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows.iter().map(|s| parse_uuid(s)).collect()
    }

    /// Every path some node is linked to, with how many nodes.
    pub fn linked_docs(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT path, count(*) FROM doc_links GROUP BY path ORDER BY path")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as usize))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Every link, as (source, relation, destination).
    pub fn edges(&self) -> Result<Vec<(Uuid, Rel, Uuid)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT src, rel, dst FROM edges ORDER BY src, rel, dst")?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .map(|(s, r, d)| Ok((parse_uuid(&s)?, parse_rel(&r)?, parse_uuid(&d)?)))
            .collect()
    }

    /// A node's key, title and state, if it exists.
    pub fn brief(&self, id: Uuid) -> Result<Option<Brief>> {
        let row: Option<(String, String, String, String)> = self
            .conn
            .prepare_cached("SELECT key, title, state, kind FROM nodes WHERE id = ?1")?
            .query_row([id.to_string()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        row.map(|(key, title, state, kind)| {
            Ok(Brief {
                key,
                title,
                state: state.parse()?,
                kind: kind.parse()?,
            })
        })
        .transpose()
    }

    /// A value kept beside the projection that a rebuild leaves alone.
    pub fn meta(&self, k: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT v FROM meta WHERE k = ?1", [k], |r| r.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, k: &str, v: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET v = ?2",
            params![k, v],
        )?;
        Ok(())
    }

    /// Events after `after` (all, without it) by anyone but `except`, counted up to `cap`:
    /// the count stops there, so it costs the same however long the log grows.
    pub fn count_events_after(
        &self,
        after: Option<Uuid>,
        except: &str,
        cap: usize,
    ) -> Result<usize> {
        let n: i64 = self
            .conn
            .prepare_cached(
                "SELECT count(*) FROM (SELECT 1 FROM events \
                 WHERE (?1 IS NULL OR id > ?1) AND by != ?2 LIMIT ?3)",
            )?
            .query_row(
                params![after.map(|a| a.to_string()), except, cap as i64],
                |r| r.get(0),
            )?;
        Ok(n as usize)
    }

    /// The events after `after` (or all of them), oldest first.
    pub fn events_after(&self, after: Option<Uuid>) -> Result<Vec<Event>> {
        // uuidv7s in their text form sort as their times do.
        let mut stmt = self
            .conn
            .prepare_cached("SELECT line FROM events WHERE ?1 IS NULL OR id > ?1 ORDER BY id")?;
        let lines: Vec<String> = stmt
            .query_map(params![after.map(|a| a.to_string())], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        lines
            .iter()
            .map(|l| serde_json::from_str(l).map_err(Error::from))
            .collect()
    }

    /// The whole log as applied, oldest first.
    pub fn all_events(&self) -> Result<Vec<Event>> {
        self.events_where("", None)
    }

    fn events_where(&self, filter: &str, id: Option<Uuid>) -> Result<Vec<Event>> {
        let sql = format!("SELECT line FROM events {filter} ORDER BY id");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let lines: Vec<String> = match id {
            Some(id) => stmt
                .query_map([id.to_string()], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?,
            None => stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?,
        };
        lines
            .iter()
            .map(|l| serde_json::from_str(l).map_err(Error::from))
            .collect()
    }

    // ---- search ---------------------------------------------------------------------------

    /// Keyword matches for an FTS5 query, best first (bm25, title weighted three to one).
    pub fn keyword(&self, fts_query: &str, recall_only: bool, limit: usize) -> Result<Vec<Uuid>> {
        let filter = if recall_only { RECALLABLE } else { "1" };
        let sql = format!(
            "SELECT n.id FROM nodes_fts JOIN nodes n ON n.rowid = nodes_fts.rowid \
             WHERE nodes_fts MATCH ?1 AND {filter} \
             ORDER BY bm25(nodes_fts, 3.0, 1.0), n.id LIMIT ?2"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let ids: Vec<String> = stmt
            .query_map(
                params![fts_query, i64::try_from(limit).unwrap_or(i64::MAX)],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        ids.iter().map(|s| parse_uuid(s)).collect()
    }

    /// Every node search may rank by meaning, with the text it is embedded from.
    pub fn texts(&self, recall_only: bool) -> Result<Vec<(Uuid, String)>> {
        let filter = if recall_only { RECALLABLE } else { "1" };
        let sql = format!("SELECT n.id, n.title, n.body FROM nodes n WHERE {filter} ORDER BY n.id");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .map(|(id, title, body)| Ok((parse_uuid(&id)?, format!("{title}\n{body}"))))
            .collect()
    }

    /// Stored embeddings with the hash they were made under.
    pub fn vectors(
        &self,
        recall_only: bool,
    ) -> Result<std::collections::HashMap<Uuid, (String, Vec<f32>)>> {
        let filter = if recall_only { RECALLABLE } else { "1" };
        let sql = format!(
            "SELECT v.node, v.hash, v.vec FROM vectors v JOIN nodes n ON n.id = v.node WHERE {filter}"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .map(|(id, hash, blob)| {
                let v = blob
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect();
                Ok((parse_uuid(&id)?, (hash, v)))
            })
            .collect()
    }

    /// Store embeddings. Derived data: written outside the log's lock, idempotent by hash.
    pub fn put_vectors(&mut self, items: &[(Uuid, String, Vec<f32>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO vectors (node, hash, vec) VALUES (?1, ?2, ?3)",
            )?;
            for (id, hash, v) in items {
                let blob: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
                stmt.execute(params![id.to_string(), hash, blob])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Ratified lessons marked as standing rules, oldest first.
    pub fn always_rules(&self) -> Result<Vec<Brief>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT key, title, state FROM nodes WHERE kind = 'lesson' AND state = 'ratified' \
             AND json_extract(props, '$.scope') = 'always' ORDER BY id",
        )?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .map(|(key, title, state)| {
                Ok(Brief {
                    key,
                    title,
                    state: state.parse()?,
                    kind: Kind::Lesson,
                })
            })
            .collect()
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let sql = format!("SELECT {NODE_COLS}, {BLOCKED} FROM nodes n ORDER BY n.id");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut nodes = Vec::new();
        while let Some(row) = rows.next()? {
            nodes.push(node_from(row)?);
        }
        Ok(Snapshot {
            nodes,
            edges: self.edges()?,
        })
    }
}

// ---- applying events ------------------------------------------------------------------------

fn apply_one(tx: &Transaction<'_>, e: &Event) -> Result<Option<String>> {
    let (node, other) = e.op.subjects();
    let inserted = tx
        .prepare_cached(
            "INSERT OR IGNORE INTO events (id, at, by, op, node, other, line) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            e.id.to_string(),
            e.at,
            e.by,
            e.op.name(),
            node.to_string(),
            other.map(|o| o.to_string()),
            serde_json::to_string(e)?,
        ])?;
    if inserted == 0 {
        return Ok(None);
    }
    match &e.op {
        Op::NodeCreate { node, data } => create(tx, e, *node, data),
        Op::NodeSet { node, data } => set(tx, e, *node, data),
        Op::NodeNote { node, .. } => {
            // The note itself lives in `events`; the node only records the activity.
            let touched = tx
                .prepare_cached("UPDATE nodes SET updated = ?2 WHERE id = ?1")?
                .execute(params![node.to_string(), e.at])?;
            if touched == 0 {
                return Ok(Some(format!(
                    "event {}: note on unknown node {node}, kept in history only",
                    e.id
                )));
            }
            Ok(None)
        }
        Op::EdgeAdd { data } => {
            if !exists(tx, data.src)? || !exists(tx, data.dst)? {
                return Ok(Some(format!(
                    "event {}: edge between unknown nodes, skipped",
                    e.id
                )));
            }
            tx.prepare_cached("INSERT OR IGNORE INTO edges (src, rel, dst) VALUES (?1, ?2, ?3)")?
                .execute(params![
                    data.src.to_string(),
                    data.rel.as_str(),
                    data.dst.to_string()
                ])?;
            Ok(None)
        }
        Op::EdgeRemove { data } => {
            tx.prepare_cached("DELETE FROM edges WHERE src = ?1 AND rel = ?2 AND dst = ?3")?
                .execute(params![
                    data.src.to_string(),
                    data.rel.as_str(),
                    data.dst.to_string()
                ])?;
            Ok(None)
        }
        Op::DocLink { node, data } => {
            if !exists(tx, *node)? {
                return Ok(Some(format!(
                    "event {}: doc link from unknown node {node}, skipped",
                    e.id
                )));
            }
            tx.prepare_cached("INSERT OR IGNORE INTO doc_links (node, path) VALUES (?1, ?2)")?
                .execute(params![node.to_string(), data.path])?;
            touch(tx, *node, &e.at)?;
            Ok(None)
        }
        Op::DocUnlink { node, data } => {
            tx.prepare_cached("DELETE FROM doc_links WHERE node = ?1 AND path = ?2")?
                .execute(params![node.to_string(), data.path])?;
            touch(tx, *node, &e.at)?;
            Ok(None)
        }
    }
}

/// A node's last change is now `at`.
fn touch(tx: &Transaction<'_>, node: Uuid, at: &str) -> Result<()> {
    tx.prepare_cached("UPDATE nodes SET updated = ?2 WHERE id = ?1")?
        .execute(params![node.to_string(), at])?;
    Ok(())
}

fn create(tx: &Transaction<'_>, e: &Event, node: Uuid, data: &NodeData) -> Result<Option<String>> {
    if exists(tx, node)? {
        return Ok(Some(format!(
            "event {}: node {node} already exists, skipped",
            e.id
        )));
    }
    let Ok(wanted) = data.key.parse::<Key>() else {
        return Ok(Some(format!(
            "event {}: bad key `{}`, skipped",
            e.id, data.key
        )));
    };
    bump_counter(tx, &wanted.prefix, wanted.number)?;
    let taken: bool = tx
        .prepare_cached("SELECT 1 FROM nodes WHERE key = ?1")?
        .query_row([&data.key], |_| Ok(()))
        .optional()?
        .is_some();
    // Two branches can both create the same key. The earlier event keeps it; the later one
    // takes the next free number. Its first key stays in its create event, for history.
    let key = if taken {
        let n = next_number(tx, &wanted.prefix)?;
        bump_counter(tx, &wanted.prefix, n)?;
        Key::new(&wanted.prefix, n).to_string()
    } else {
        data.key.clone()
    };
    tx.prepare_cached(
        "INSERT INTO nodes (id, key, kind, title, body, state, priority, props, created, updated, created_by) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
    )?
    .execute(params![
        node.to_string(),
        key,
        data.kind.as_str(),
        data.title,
        data.body,
        data.state.as_str(),
        data.priority.get(),
        serde_json::to_string(&data.props)?,
        e.at,
        e.by,
    ])?;
    tx.prepare_cached(
        "INSERT INTO nodes_fts (rowid, title, body) VALUES (last_insert_rowid(), ?1, ?2)",
    )?
    .execute(params![data.title, data.body])?;
    Ok(None)
}

fn set(tx: &Transaction<'_>, e: &Event, node: Uuid, p: &NodePatch) -> Result<Option<String>> {
    let current: Option<(i64, String, String, String)> = tx
        .prepare_cached("SELECT rowid, title, body, props FROM nodes WHERE id = ?1")?
        .query_row([node.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    let Some((rowid, old_title, old_body, props)) = current else {
        return Ok(Some(format!(
            "event {}: set on unknown node {node}, skipped",
            e.id
        )));
    };
    let merged = match &p.props {
        Some(patch) => {
            let mut current: Map<String, Value> = serde_json::from_str(&props).unwrap_or_default();
            for (k, v) in patch {
                if v.is_null() {
                    current.remove(k);
                } else {
                    current.insert(k.clone(), v.clone());
                }
            }
            Some(serde_json::to_string(&current)?)
        }
        None => None,
    };
    tx.prepare_cached(
        "UPDATE nodes SET \
           title = coalesce(?2, title), body = coalesce(?3, body), state = coalesce(?4, state), \
           priority = coalesce(?5, priority), props = coalesce(?6, props), updated = ?7 \
         WHERE id = ?1",
    )?
    .execute(params![
        node.to_string(),
        p.title,
        p.body,
        p.state.map(State::as_str),
        p.priority.map(|x| x.get()),
        merged,
        e.at,
    ])?;
    if p.title.is_some() || p.body.is_some() {
        tx.prepare_cached(
            "INSERT INTO nodes_fts (nodes_fts, rowid, title, body) VALUES ('delete', ?1, ?2, ?3)",
        )?
        .execute(params![rowid, old_title, old_body])?;
        tx.prepare_cached("INSERT INTO nodes_fts (rowid, title, body) VALUES (?1, ?2, ?3)")?
            .execute(params![
                rowid,
                p.title.as_deref().unwrap_or(&old_title),
                p.body.as_deref().unwrap_or(&old_body)
            ])?;
    }
    Ok(None)
}

fn exists(tx: &Transaction<'_>, node: Uuid) -> Result<bool> {
    Ok(tx
        .prepare_cached("SELECT 1 FROM nodes WHERE id = ?1")?
        .query_row([node.to_string()], |_| Ok(()))
        .optional()?
        .is_some())
}

fn bump_counter(tx: &Transaction<'_>, prefix: &str, n: u64) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO counters (prefix, last) VALUES (?1, ?2) \
         ON CONFLICT (prefix) DO UPDATE SET last = max(last, excluded.last)",
    )?
    .execute(params![prefix, i64::try_from(n).unwrap_or(i64::MAX)])?;
    Ok(())
}

fn next_number(conn: &Connection, prefix: &str) -> rusqlite::Result<u64> {
    let last: Option<i64> = conn
        .prepare_cached("SELECT last FROM counters WHERE prefix = ?1")?
        .query_row([prefix], |r| r.get(0))
        .optional()?;
    Ok(u64::try_from(last.unwrap_or(0)).unwrap_or(0) + 1)
}

// ---- reading rows ---------------------------------------------------------------------------

fn node_from(row: &rusqlite::Row<'_>) -> Result<Node> {
    // Columns that are only parsed are borrowed, not copied: this runs once per row of a list.
    let text =
        |i: usize| -> Result<&str> { Ok(row.get_ref(i)?.as_str().map_err(rusqlite::Error::from)?) };
    let props = text(7)?;
    Ok(Node {
        id: parse_uuid(text(0)?)?,
        key: row.get(1)?,
        kind: text(2)?.parse()?,
        title: row.get(3)?,
        body: row.get(4)?,
        state: text(5)?.parse()?,
        priority: row.get(6)?,
        props: if props == "{}" {
            Map::new()
        } else {
            serde_json::from_str(props)?
        },
        created: row.get(8)?,
        updated: row.get(9)?,
        created_by: row.get(10)?,
        blocked: row.get(11)?,
    })
}

/// A row of [`SUMMARY_COLS`] as a summary.
fn summary_from(row: &rusqlite::Row) -> Result<Summary> {
    let text =
        |i: usize| -> Result<&str> { Ok(row.get_ref(i)?.as_str().map_err(rusqlite::Error::from)?) };
    Ok(Summary {
        id: parse_uuid(text(0)?)?,
        key: row.get(1)?,
        kind: text(2)?.parse()?,
        title: row.get(3)?,
        state: text(4)?.parse()?,
        priority: row.get(5)?,
        updated: row.get(6)?,
        blocked: row.get(7)?,
        standing: row.get(8)?,
        parent: row
            .get_ref(9)?
            .as_str_or_null()
            .map_err(rusqlite::Error::from)?
            .map(parse_uuid)
            .transpose()?,
    })
}

fn parse_uuid(s: &str) -> Result<Uuid> {
    s.parse()
        .map_err(|_| Error::Invalid(format!("store holds a bad id `{s}`; run `livtk rebuild`")))
}

fn parse_rel(s: &str) -> Result<Rel> {
    serde_json::from_value(Value::String(s.to_owned())).map_err(|_| {
        Error::Invalid(format!(
            "store holds an unknown relation `{s}`; run `livtk rebuild`"
        ))
    })
}
