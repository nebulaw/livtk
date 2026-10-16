# livtk: plan

Status: milestones 1 to 4 done; milestone 5 parts 1 and 2, the command palette and search
(section 17) and the activity feed (section 19), done; the graph waits for the developer's go.

A local, durable task and memory system for Claude Code, plus a desktop app to drive it.
Reusable in any project as a Claude Code plugin. No API calls, no servers, no keys:
Claude Code is the only intelligence, everything else is plain local tooling.

## 1. Goals and non-goals

**Goals**

- Durable work: tasks, dependencies and history survive sessions, crashes, compaction and git
  branches.
- Human gates enforced in code: Claude plans, the human acknowledges; Claude proposes lessons,
  the human ratifies.
- Efficient memory: Claude sees only the rules and decisions relevant to the current prompt,
  inside a fixed token budget.
- Fast: hooks run on every prompt and tool call, so they must cost milliseconds.
- One model for everything (tasks, lessons, decisions, findings), shown as a list, a board and a
  graph.
- Reusable: install into any project with `/plugin install`; per-project data.

**Non-goals (for now)**

- Multi-user sync, a server, a web UI, cloud anything.
- Calling any model API. Embeddings run locally.
- Autonomous self-modification. The OS learns only through human-ratified lessons.

## 2. Principles

1. **One truth.** The event log is the truth. Everything else (the SQLite database, the search
   index, the app's state) is a projection that can be rebuilt from it.
2. **Thin surfaces.** All logic lives in `livtk-core`. The CLI, the hooks and the app are thin
   adapters over the same API. A rule enforced once is enforced everywhere.
3. **Generic over specific.** A small, fixed set of event operations works for every kind of
   node. New kinds and fields need no new operations and no data migration.
4. **Deterministic.** The same events always give the same state and the same bytes.
5. **Fail safe.** A hook that hits an internal error exits 0 and stays silent; it never breaks a
   session. Only the human gates block.
6. **Measured.** Every performance claim has a benchmark (section 9).

## 3. Architecture

```
tasks/                       (git repo = Claude Code plugin + marketplace)
├── .claude-plugin/          plugin.json, marketplace.json
├── bin/livtk                wrapper: finds the installed livtk binary, explains if missing
├── hooks/hooks.json         every hook is `livtk hook <event>`
├── skills/  agents/         plan, work, review, remember / implementer, refuter, researcher
├── crates/
│   ├── livtk-core/          the library: model, log, store, gates, search
│   ├── livtk-cli/           the `livtk` binary: commands + hook adapters + output
│   └── livtk-app/           the GPUI desktop app (in-process, uses livtk-core)
└── docs/
```

Crates stay few. Inside `livtk-core`, modules carry the boundaries; a module becomes a crate only
when a real boundary hurts (compile time, an optional heavy dependency).

| Module in livtk-core | Responsibility | Depends on |
|---|---|---|
| `model` | ids, keys, kinds, states, the transition table, validation. Pure, no IO | nothing |
| `event` | the event format, versioning, (de)serialisation | `model` |
| `log` | append, read, catch-up, change detection on `events.jsonl` | `event` |
| `store` | SQLite projection: apply events, queries, rebuild | `event`, `model` |
| `workspace` | the public API: open, commands, queries, the write lock | all above |
| `search` | FTS5 ranking, embeddings, hybrid fusion (milestone 3) | `store` |

Embeddings sit behind a cargo feature so the core builds without them.

## 4. Data model

Everything is a **node**, linked by **edges**, changed by **events**.

**Node**: `id` (uuidv7, internal, never changes), `key` (readable, e.g. `LIV-12`), `kind`,
`title`, `body` (markdown), `state`, `priority` (1 to 4, like Linear), `props` (kind-specific
fields, JSON), `created`, `updated`, `created_by`.

**Edge**: `src`, `dst`, `rel`. Relations: `depends_on`, `relates`, `supersedes`, `learned_from`,
`parent`.

**Kinds**

| Kind | States | Milestone |
|---|---|---|
| task | backlog → planned → acked → doing → review → done; parked, canceled | 1 |
| lesson | proposed → ratified / rejected. A ratified lesson is a rule | 3 |
| decision | proposed → accepted → superseded | 3 |
| finding | open → recorded | 3 |
| note | none | 3 |

**Task transitions**

| From | To |
|---|---|
| backlog | planned, parked, canceled |
| planned | **acked** (human only), backlog, parked, canceled |
| acked | doing, planned, parked, canceled |
| doing | review, acked (pause), parked, canceled |
| review | done, doing |
| done | doing (reopen) |
| parked | backlog, planned |
| canceled | backlog |

**Gate rules (in `livtk-core`)**

- Only two steps need actor `human`: acknowledging a task (`planned → acked`) and ratifying or
  rejecting a lesson. Every other step, including marking a task `done`, is open to Claude. The
  list is configurable per project.
- `acked → doing` needs every `depends_on` node to be `done`.
- Changing the title or body of an `acked` task moves it back to `planned`: the plan changed, so
  it needs a fresh acknowledgement.
- A `depends_on` edge that would close a cycle is refused.

**Actor.** The app is always `human`. The CLI is `claude` when `CLAUDECODE=1` is set (Claude Code
sets it in every shell it runs), otherwise `human`. The plugin names subagents
(`claude:refuter`) with a hidden `--agent` flag, which a human-run command refuses. This is a guard rail, not security: milestone 2 adds a hook that blocks
Claude from running human-only commands or stripping the variable. The human is the root of
trust.

## 5. Storage and durability

```
<project>/.livtk/
├── events.jsonl      the truth, committed to git
├── config.toml       project settings, committed
├── .gitattributes    events.jsonl merge=union
├── .gitignore        livtk.db*, lock
├── livtk.db          SQLite: the event log plus its projection (WAL), rebuildable
└── lock              write lock
```

**Why an event log instead of a snapshot.** A snapshot file (one line per task) conflicts in
git whenever two branches touch the same task, and a conflict marker inside JSONL breaks the
import. An append-only log with git's `union` merge never conflicts: both branches' lines are
kept, and replay sorts them by time. The log is also the history, the activity feed and the
undo source.

**Event format (one line each, versioned)**

```json
{"v":1,"id":"<uuidv7>","at":"2026-10-16T12:00:00.000Z","by":"human","op":"node.set","node":"<uuid>","data":{"state":"acked"}}
```

| Op | Data |
|---|---|
| `node.create` | kind, key, title, body, state, priority, props |
| `node.set` | any subset of fields (field-level last-writer-wins) |
| `edge.add` / `edge.remove` | src, dst, rel |

Five operations cover every kind. Validation happens when an event is written; replay only
applies, so replay is total and can never fail on a merged log. Ordering is by uuidv7, which is
time-ordered. Unknown ops or a newer `v` are skipped with a warning (forward compatibility).

**Readable keys after a merge.** If two branches both create `LIV-13`, replay keeps the earlier
one and gives the later one the next free number, deterministically. `LIV-13` always means the
node that kept it; the other node's first key stays in its create event, and its history shows
"created as LIV-13".

**Write path (the one critical invariant)**

1. Take the exclusive file lock (`std::fs::File::lock`).
2. Catch up: apply any log lines the database has not seen.
3. Validate the command against the current state (gates, transitions, cycles).
4. Append the event to `events.jsonl` and fsync. This is the commit point.
5. In one SQLite transaction: insert the event into `events`, update the projection, record the
   new log offset.
6. Release the lock.

A crash after step 4 loses nothing: the next open catches up from the log.

**Read path.** No lock. Compare the log's size and a hash of its head with what the database
recorded. Appended lines are applied incrementally; any other change (git pull, checkout,
rebase) triggers a full rebuild. Reads use SQLite WAL snapshots, so the app and the CLI never
block each other.

**What the database holds.** The full event log as a table (`events`: every event, indexed by
time and by node), so history, the activity feed and undo are plain queries. Beside it, the
projection built from those events: `nodes`, `edges`, `counters` (the last number per prefix),
`meta` (format version, log offset and head hash) and, from milestone 3, the FTS5 index and the
vectors. The JSONL file is the portable copy for git; the database carries the same events plus
everything derived from them.

**SQLite.** WAL, `synchronous=NORMAL` (safe because the log is fsynced first),
`foreign_keys=ON`, `busy_timeout=5000`. Because the database is only a projection, a schema change
is a version bump plus a rebuild. There are never data migrations. Only the event format needs
care, and it is versioned.

**Growth.** About 300 bytes per event. 100k events is about 30 MB and rebuilds in well under a
second. `livtk compact` (snapshot plus a fresh log) is designed for later and not built now.

## 6. Memory (milestone 3)

- **Lessons are rules.** Claude proposes a lesson; the human ratifies it; a ratified lesson is a
  binding rule. There is no separate rule kind and no copying into CLAUDE.md.
- **Two kinds of rules.** `always` rules (a handful) are injected at session start. All others
  are recalled by relevance.
- **Recall per prompt.** Hybrid ranking: SQLite FTS5 (BM25) and model2vec embeddings, fused with
  reciprocal rank fusion. Only results above a threshold are injected, inside a fixed token
  budget (default about 400 tokens).
- **Vectors as plain BLOBs**, compared by brute-force cosine in Rust. For tens of thousands of
  nodes that takes a few milliseconds and avoids the pre-1.0 `sqlite-vec` dependency. Swap later
  if it ever matters.
- **Embedding model** behind a trait, `potion-base-8M` by default. Downloaded once by
  `livtk embed setup`, then fully offline.
- **CLAUDE.md** shrinks to a few lines that say the OS exists and how to use it.

## 7. Claude Code integration (milestone 2)

**Install.** `cargo install --locked --path crates/livtk-cli` once. The plugin's `bin/livtk` wrapper
finds the binary and prints the install line if it is missing. Prebuilt releases come later.

**Hooks**: all `livtk hook <event>`, each reading the hook JSON on stdin.

| Event | Does |
|---|---|
| SessionStart | Brief: tasks in progress, tasks waiting for your ack, lessons waiting for ratification, `always` rules |
| UserPromptSubmit | Recall (milestone 3); spot corrections and nudge Claude to propose a lesson |
| PreToolUse (Bash) | Block human-only `livtk` commands and environment tampering |
| PreCompact | Keep the tasks in progress in context |
| SubagentStop / Stop | Run the project's check command, only if files changed and only for agents that write code |
| PostToolUse (Edit/Write) | Run the project's format command on the edited file |

Commands for checking and formatting come from `.livtk/config.toml`, so the OS stays language
neutral (Rust today, anything tomorrow).

**Skills**: `plan` (break work into tasks with acceptance checks), `work` (next → start → do →
review), `review` (the refuter), `remember` (propose a lesson or decision).

**Agents**: `implementer`, `refuter` (sees only the diff and the acceptance check), `researcher`.
Project-specific specialists live in the project, not in the OS.

**Claude Code's own todo list** stays for steps inside one session. `livtk` is for work that must
survive the session.

## 8. The app (milestones 4 and 5)

- **GPUI + gpui-component**, pinned to an exact git revision. All GPUI code is in `livtk-app`, so
  framework churn never touches the core.
- **In-process.** The app links `livtk-core` directly; no server. A file watcher on `events.jsonl`
  triggers catch-up and a re-render within about 100 ms of a change Claude makes.
- **Projects** listed in `~/.config/livtk/projects.toml`; the sidebar switches between them.
- **Views**
  - Inbox: everything waiting for you (acks, lessons to ratify).
  - List (grouped by state) and Board, keyboard-first like Linear (`j`/`k`, Enter, shortcuts).
  - Task detail: markdown body, dependencies, the event timeline.
  - Lessons and rules.
  - Graph: every node and edge, force-directed, drawn on a GPU canvas (milestone 5).
  - Search and a command palette (milestone 5).
- Every edit goes through the same `livtk-core` commands with actor `human`.

## 9. Quality

- **Tests from the outside.**
  - Unit tests for the transition table and gates.
  - Property tests (`proptest`): any interleaving or merge of the same events gives the same
    state; export → import gives identical bytes.
  - Multi-process tests: many `livtk` processes writing at once lose nothing and never duplicate a
    key.
  - CLI tests with `assert_cmd`.
- **Benchmarks with budgets** on a 10k-node project:

  | Path | Budget |
  |---|---|
  | `livtk` command, warm | < 10 ms |
  | any hook | < 20 ms |
  | full rebuild, 100k events | < 1 s |
  | recall, 10k nodes | < 15 ms |

- **Lint**: `rustfmt`, `clippy -D warnings`, `cargo deny`. No `unwrap` on runtime paths. Errors
  are typed (`thiserror`) in the core and carry a fix hint.
- **Pinned toolchain** (`rust-toolchain.toml`), `[workspace.dependencies]` for versions, edition
  2024.

## 10. Milestones

Each one ends with something you can use, and starts only after your acknowledgement.

| # | Delivers | Done when |
|---|---|---|
| 1 | `livtk-core` + `livtk` CLI: tasks, dependencies, gates, the log, the projection | The tests in section 9 pass; you create, ack and move tasks in a terminal |
| 2 | Plugin: hooks, skills, agents | Installed in `livspeil`; a session starts with the brief; Claude cannot ack |
| 3 | Memory: lessons, decisions, findings, recall, embeddings | A ratified rule shows up when a matching prompt is typed; recall stays within budget |
| 4 | App v1: inbox, list, board, detail, ack and ratify | You run a whole day of work from the app |
| 5 | App v2: graph, search, palette, activity feed | Graph of a real project renders smoothly |
| 6 | Team: keys that never change, who did what, hosted (section 18) | Two people create tasks at once and every key stays as created |

Milestone 5 is built in parts, each acknowledged: the command palette and search first
(section 17), then the activity feed, then the graph.

Later, only when needed: `livtk compact`, prebuilt binaries, cross-project views, an MCP server.

## 11. Risks

| Risk | Mitigation |
|---|---|
| GPUI breaks between versions | Pinned revision; UI isolated in `livtk-app` |
| Event log grows | Compaction is designed (section 5) |
| Keys renumbered after a merge | Deterministic renumbering; the first key stays in the log. Fine for one person; for a team it is the main risk, solved in milestone 6 |
| Claude bypasses a human gate | Gate in core, guard hook, and the human as root of trust |
| Static embeddings recall poorly | Hybrid with BM25; the embedder is swappable |

## 12. Decisions (developer, 2026-10-16)

1. The event log is the git truth (section 5), and every event is also stored in the database.
2. The CLI is called `livtk`; per-project data lives in `.livtk/`.
3. Human-only steps: acknowledge and ratify. Nothing else, including `done`.
4. A ratified lesson is the rule itself; no separate copy.
5. Install with `cargo install` for now; prebuilt binaries later.
6. The app is GPUI + gpui-component.

## 13. Milestone 1 in detail

**Workspace.** Edition 2024, pinned toolchain, versions in `[workspace.dependencies]`.
Dependencies: `rusqlite` (bundled SQLite), `uuid` (v7), `serde` + `serde_json`, `jiff` (time),
`thiserror` (core errors), `clap` (CLI), `anyhow` (CLI only). Dev: `proptest`, `assert_cmd`,
`tempfile`. Nothing else.

**`livtk-core` public API** (everything the CLI and the app may call)

```rust
Workspace::init(dir, prefix) -> Result<Workspace>
Workspace::open(dir) -> Result<Workspace>         // finds .livtk/ upward, catches up
ws.execute(Command, Actor) -> Result<Vec<Event>>  // the only write entry point
ws.node(KeyOrId) / ws.list(Filter) / ws.next() / ws.history(KeyOrId) / ws.deps(KeyOrId)
```

`Command` is an enum (`CreateTask`, `Edit`, `Move`, `SetPriority`, `Link`, `Unlink`). Each
command is validated, then lowered to the five generic events. Queries return plain structs that
derive `Serialize`, so `--json` output and the app share one shape.

**`livtk` commands**

| Command | Does |
|---|---|
| `livtk init [--prefix LIV]` | Create `.livtk/` with config, gitignore, gitattributes |
| `livtk new "title" [--body ..] [--priority 1-4] [--after KEY]` | New task in backlog |
| `livtk list [--state ..] [--all]` / `livtk board` | Open tasks grouped by state |
| `livtk show KEY` | Task, its dependencies, its history |
| `livtk edit KEY [--title ..] [--body ..]` | Edit (an acked task returns to planned) |
| `livtk move KEY STATE` | Any legal transition |
| `livtk ack KEY` | Human only |
| `livtk start KEY` / `livtk done KEY` | Shortcuts for `move` |
| `livtk dep add KEY --on KEY` / `dep rm` | Dependencies, cycles refused |
| `livtk next` | The best runnable task |
| `livtk log [KEY]` | Event history |
| `livtk rebuild` | Rebuild the database from the log |

Every command takes `--json`. Errors print one line saying what is wrong and how to fix it.

**Tests that close the milestone**

1. Transition table: every legal move passes, every illegal one fails with its reason.
2. Gates: `ack` as `claude` is refused; `start` with an unfinished dependency is refused; editing
   an acked task returns it to planned; a dependency cycle is refused.
3. Replay determinism (property test): shuffling and merging the same event lines always yields
   the same database state.
4. Merge: two branches that both created the same key replay to distinct keys, and the old key
   still resolves.
5. Crash safety: an event appended to the log but never applied is picked up on the next open.
6. Concurrency: 8 processes x 25 `livtk new` give exactly 200 tasks, 200 unique keys, a valid log.
7. Rebuild: `livtk rebuild` reproduces the same state as incremental apply.
8. Bench: `livtk list` on 10k tasks under 10 ms warm.

Lint clean (`fmt`, `clippy -D warnings`). One commit for the scaffold, then one per working
slice, all local.

## 14. Milestone 2 in detail: the Claude Code plugin

**Layout.** The plugin lives in `plugin/`, so installing it never copies the Rust sources or
`target/`. The repo root carries the marketplace that points at it.

```
tasks/
├── .claude-plugin/marketplace.json     one plugin, source ./plugin
└── plugin/
    ├── .claude-plugin/plugin.json
    ├── hooks/hooks.json                every hook is `livtk hook <event>`, exec form
    ├── skills/plan/  skills/work/  skills/review/
    └── agents/implementer.md  refuter.md  researcher.md
```

No `bin/` wrapper: hooks and Claude call the `livtk` installed by `cargo install`. If it is
missing, Claude Code shows a hook error that names it, and nothing else breaks.

**`livtk hook <event>`.** Reads the hook JSON on stdin and answers in the documented shapes.
Outside a livtk project it prints nothing and exits 0, so the plugin can be enabled everywhere.
An internal error never blocks a session: it exits 0 and stays silent.

| Hook | Does |
|---|---|
| SessionStart (startup, resume, clear, compact) | The brief, under 1,500 characters: work in progress with its last event, tasks waiting for the human's ack, the next task, and how to drive livtk. After compaction this restores the thread, so no PreCompact hook is needed |
| SubagentStart | A shorter brief for the subagent: the task in progress and the rules it must follow |
| PreToolUse (Bash) | Denies a command that runs a human-only livtk step (`ack`, `move .. acked`) or touches `CLAUDECODE`. When a subagent runs `livtk`, rewrites the command to sign its events: livtk's own hidden `--agent <name>` flag goes right after each `livtk` the shell runs, so the command still starts with `livtk`, the user's permission rules still match it, and no permission decision is returned. (A first version put `LIVTK_AGENT=<name>` in front; the live check showed that breaks `livtk *` allow rules, and subagent shells carry no variable naming the agent, so the hook must pass it.) |
| PreToolUse (Edit, Write) | Denies hand edits to anything inside `.livtk/`: the log changes only through livtk |
| PostToolUse (Edit, Write) | Runs the project's format command on the edited file, if configured |
| SubagentStop | Runs the project's check command before a configured agent (by default the implementer) may stop; blocks with the failing output, once (no loop: `stop_hook_active`) |

The Bash guard matches text, so it is a guard rail, not security: a command that merely mentions
`livtk ack` in quotes is also denied, and Claude is told to use the Grep tool instead. The human
stays the root of trust.

**Project settings**, all optional, in `.livtk/config.toml`:

```toml
[hooks]
format = ["rustfmt", "--edition", "2024", "{file}"]
format_extensions = ["rs"]
check = ["cargo", "check", "--quiet"]
check_agents = ["livtk:implementer"]
```

**Skills** (namespaced `livtk:`)

| Skill | What Claude does |
|---|---|
| `plan` | Reads the code, writes tasks with a body of Goal, Approach, Files, Acceptance check; adds dependencies; moves them to planned; then stops and asks the human to acknowledge |
| `work` | `livtk next` → `start` → do the work → verify against the acceptance check → `move review` → hand to the refuter → `done`, or back to `doing` with a note on what failed |
| `review` | Runs the refuter on a task in review and records its verdict |

**Agents**: `implementer` (writes code within one task), `refuter` (read-only; sees the diff
and the acceptance check, tries to prove the task is not done), `researcher` (read-only).

**One addition to the event format: notes.** Work needs a place to say "tried X, failed because
Y" without editing the plan (which would send it back for acknowledgement). A sixth operation,
`node.note {node, text}`, records a note in the history; it changes no state. `livtk note KEY
"text"` writes one; `show` and the app's timeline display them. Each event now carries the
format version its operation needs (`node.note` is version 2, everything else stays 1), so older
logs keep their exact bytes and an older livtk skips a note with an "update livtk" warning.

**Tests**

1. Each hook as a process: JSON in, JSON and exit code out, for every row of the table.
2. The guard denies `livtk ack 1`, `cd x && livtk --json move 2 acked`, `env -u CLAUDECODE
   livtk list`, a nested `sh -c "livtk ack 1"`, and an Edit of `.livtk/events.jsonl`; it allows
   `livtk list`, `git add .livtk`, and edits elsewhere.
3. SubagentStop blocks on a failing check, not when `stop_hook_active` is set, and never for an
   agent outside `check_agents`.
4. Outside a project every hook is silent.
5. Each hook under 20 ms.
6. `claude plugin validate` passes.

**Done when** the plugin is installed from the local marketplace, `livspeil` has a `.livtk/`,
and in a real session: the brief appears, Claude plans tasks and stops for your ack, Claude
cannot ack, and a subagent's events carry its name.

## 15. Milestone 3 in detail: memory

**Goal.** Claude learns from corrections and keeps decisions, but only what the human ratifies
binds it, and each prompt carries only the few rules that matter to it.

**Three new kinds**

| Kind | Key | States | Human only |
|---|---|---|---|
| lesson | `LES-n` | proposed → ratified / rejected; ratified → retired; rejected → proposed | every move out of proposed, and retiring |
| decision | `DEC-n` | proposed → accepted / rejected; accepted → superseded | accepting, rejecting |
| finding | `FND-n` | recorded | nothing: research Claude may keep freely |

- A lesson's title is the rule in one sentence; its body says why, with the evidence.
- Editing a ratified lesson or an accepted decision returns it to proposed, like an acknowledged
  plan.
- `--supersedes KEY` links a new lesson or decision to the one it replaces; ratifying or
  accepting the new one retires or supersedes the old one in the same step.
- A ratified lesson is `recall` by default (shown when relevant). `livtk ratify KEY --always`
  makes it a standing rule shown at every session start. Only the human sets either.

**Commands**

| Command | Does |
|---|---|
| `livtk lesson "rule" --why .. [--from KEY] [--supersedes KEY]` | Propose a lesson |
| `livtk decision "title" --body .. [--from KEY] [--supersedes KEY]` | Propose a decision |
| `livtk finding "title" --body .. [--from KEY]` | Record a finding |
| `livtk ratify KEY [--always]`, `reject`, `retire`, `accept` | Human only |
| `livtk inbox` | Everything waiting for the human: plans, lessons, decisions |
| `livtk list --kind lesson` (task, lesson, decision, finding) | Lists by kind |
| `livtk search "query"` | Ranked search over every node, for people and the app |
| `livtk recall "query"` | What the prompt hook injects: ratified recall lessons, accepted decisions and findings that pass the relevance bar, within a fixed budget |
| `livtk embed setup` / `status` | Download the embedding model once; show whether it is in use |

**Search.** Two rankings fused with reciprocal rank fusion (k = 60):

1. **Keywords**: SQLite FTS5 over title and body (porter stemming, title weighted higher),
   kept in step with the store as events apply.
2. **Meaning**: model2vec `potion-retrieval-32M` embeddings (512 dimensions), stored as blobs beside
   the nodes, compared by cosine in Rust. Computed lazily for new or changed nodes (keyed by a
   hash of their text), kept across rebuilds, so a rebuild never re-embeds.

Without the model, search is keywords only; nothing else changes. Measured on this machine in a
fresh process: `potion-retrieval-32M` (the default, tuned for retrieval, 128 MB) loads in 80 ms
and embeds a prompt in 0.5 ms and 1,000 documents in 9 ms; `potion-base-8M` (30 MB) loads in
29 ms and can be chosen in the config (`[memory] model`).

**The model download.** `livtk embed setup` fetches three files (128 MB) with the system `curl`
into `~/.cache/livtk/models/`, so livtk itself carries no network code
(`model2vec-rs` with `local-only` and the pure-Rust tokenizer; both MIT or Apache-2.0).

**Hooks**

| Hook | Adds |
|---|---|
| UserPromptSubmit (new) | Recall for the prompt: at most 3 items that pass the relevance bar, about 400 tokens, as `KEY rule` lines. When the prompt reads as a correction or a standing rule ("from now on", "never do", "I told you"), a nudge to propose a lesson. Pastes over 4,000 characters get no nudge |
| SessionStart | The always rules, and a count of what waits in the inbox |
| PreToolUse (Bash) | `ratify`, `reject`, `retire`, `accept`, and moves into those states, join `ack` as human only |

**Skills.** New `remember`: when and how to propose a lesson, decision or finding. `plan` starts
with `livtk recall` on the goal.

**Store.** Schema version 2 (FTS and vectors), so existing stores rebuild by themselves on the
next open. Events that create a new kind or move into a new state carry format version 3, so an
older livtk skips them with an "update livtk" warning; every existing event keeps its bytes.

**Tests**

1. Transition tables and human gates for each new kind; supersede on ratify and accept; editing
   a ratified lesson returns it to proposed.
2. Keyword search: stemming, title before body, ranking stability.
3. Recall on a fixture of about 30 rules: each question finds its rule; unrelated prompts inject
   nothing. Run with the model when it is present, keywords only otherwise; both must pass.
4. Vectors: kept across rebuild; a changed lesson re-embeds; a missing model degrades cleanly.
5. Hooks: recall injects the right rule and stays silent otherwise; the correction nudge fires on
   corrections and not on ordinary requests; the brief lists always rules and the inbox.
6. Guard: the new human verbs and states are refused.
7. Speed: the prompt hook under 100 ms with the model and under 15 ms without; search on 10k
   nodes under 50 ms once vectors exist.

**Decisions (developer, 2026-10-16):** keys per kind (`LES`, `DEC`, `FND`); `potion-retrieval-32M`
by default with `potion-base-8M` as the option; download with the system `curl`; only the human
marks a lesson `--always`.

**What measuring changed.** Exploring recall with the real model on a fixture of 12 rules showed
that a fixed bar of 0.45 recalls almost nothing: with `potion-retrieval-32M` true matches score
0.19 to 0.48 and unrelated prompts at most 0.17, while `potion-base-8M` lets unrelated prompts
reach 0.29. The bar is therefore per model (0.25 and 0.30, `search::cosine_bar`). The model ranks
the right rule first for about 9 prompts in 12 and is confidently wrong on the rest, whatever
text is embedded (rule alone or rule and reasons), so the tests assert that rate and that
unrelated prompts recall nothing, not a perfect mapping. Of the prompt hook's 92 ms, the model
load is 80: reading the 128 MB matrix (27 ms), parsing the tokenizer (28 ms) and converting the
matrix (25 ms); embedding the prompt takes 0.05 ms. Reading only the rows a prompt needs would
bring the hook to about 35 ms (a possible follow-up).

**Done when** in a real session: a correction leads Claude to propose a lesson; you ratify it;
a later, differently worded prompt brings that rule back into context.

## 16. Milestone 4 in detail: the app

**Goal.** The human's control surface: see the work, acknowledge plans, ratify lessons and accept
decisions without a terminal, and watch Claude's changes arrive live.

**Measured first (2026-10-16, this machine: Hyprland on Wayland, RTX 5070 Ti).** A spike with
`gpui-kit` 0.7.3 listing a real livtk project's tasks: renders correctly; first frame 0.74 s after
launch (mostly Vulkan start-up); 229 MB resident; 54 MB binary; 3 min cold release build.

**Stack.** `gpui-kit = "=0.7.3"` (Apache-2.0): GPUI, GPUI Base, GPUI Component and default
assets in one pinned dependency. It pins `gpui-pre = "=0.3.9"`, a published snapshot of Zed's
GPUI rather than the official `gpui` crate, so versions are pinned exactly and upgraded
deliberately, in their own commit.

**Shape**

- A separate binary, `livtk-app` (crate `crates/livtk-app`), so the CLI and the hooks stay small
  and fast. The workspace's `default-members` leave it out, so `cargo test` stays quick; it is
  built and tested with `-p livtk-app`.
- In-process: the app links `livtk-core` and acts as the human through the same commands as the
  CLI. No server.
- **Live updates by polling.** Every 200 ms the app calls `Workspace::refresh` (one stat and two
  small reads when nothing changed). Polling sees every kind of change, including git replacing
  the file on checkout, which a file watch can miss. A rebuild after a large merge runs off the
  main thread.
- **Projects.** `livtk init` records the project in `~/.config/livtk/projects.toml`; the app lists
  them in its sidebar and can add one with a folder picker. `livtk-app [DIR]` opens one directly.

**Views (v1)**

| View | What it shows and does |
|---|---|
| Sidebar | Projects; Inbox (with a count), Tasks, Lessons, Decisions, Findings |
| Inbox | Everything waiting for the human, with its action: Acknowledge, Ratify (or as a standing rule), Accept, Reject |
| Tasks | A list grouped by state, or a board with a column per state; blocked and priority marked |
| Lessons, Decisions, Findings | Lists grouped by state |
| Detail | Title, state, priority, the body as markdown, links, and the timeline of history and notes; actions for the node's legal next states; add a note |
| New task | Title, priority, body |

Keyboard first, like Linear: `j`/`k` move, `Enter` opens, `Esc` closes, `a` acknowledges, `r`
ratifies, `n` creates a task, `1` to `4` set the priority, `g` then `i`, `t`, `l` jumps to the
Inbox, Tasks or Lessons. Lists are virtualised, so 10,000 tasks scroll as smoothly as ten.

**Look.** Linear's calm density, and nothing borrowed from Livspeil's glass: concentrated on
efficiency, one typeface, quiet borders, generous but tight spacing. Visual cues carry meaning: a
coloured state mark beside every key, priority as a small bar glyph, blocked work dimmed with a
lock, what waits for the human highlighted in the accent colour. Transitions are short and
purposeful (about 150 ms, ease out): the detail panel slides in, a row acknowledged from the
Inbox fades away, the selection glides, new events from Claude flash once.

**Not in v1** (milestone 5): the graph view, search and the command palette, the activity feed.

**Tests**

1. Headless UI tests with GPUI Kit's test harness: the app opens a temporary project; pressing `a`
   on a planned task acknowledges it, and the core shows the event by `human`; ratifying from the
   Inbox with the standing-rule option marks it `always`; illegal actions are not offered.
2. Live update: a change written through `livtk-core` from outside appears after a refresh tick.
3. Speed: opening a project of 10,000 tasks shows the first frame under 300 ms after start-up;
   a refresh with nothing new costs under 1 ms.

**Decisions (developer, 2026-10-16):** a separate `livtk-app` binary; polling every 200 ms;
`livtk init` registers projects; Linear's calm density with visual cues and transitions, no glass.

**What building it found.** The app's model tests caught a core bug before any UI existed:
`refresh` reported changes only when it had to catch the store up, but the CLI applies its own
writes to the shared store, so the app would never have seen Claude's work (fixed in the core:
each workspace remembers the log position it last showed). `gpui-kit`'s own `test-support` drags
in a GPUI build whose pinned macOS crates clash with ours, so the tests enable `test-support` on
`gpui-pre` directly. Dependency debug info is off in the dev profile: it was most of an 11 GB
target directory. Measured: opening a 10,000-task project and listing it 14.6 ms, an idle refresh
28 µs. From the app, the developer ratified, acknowledged and accepted the demo's inbox; each event
reads `human`.

**Done when** you run your day's acknowledging and ratifying from the app, and Claude's changes
show up in it while it is open.

## 17. Milestone 5, part 1 in detail: the command palette and search

**Goal.** One keyboard surface, as Linear's Cmd+K: find any task, lesson, decision or finding,
and run any action, without the mouse.

**Measure first.** Search over every node has not been measured at 10,000 nodes. Ranking by
meaning reads every node's text and vector from SQLite on each call (10,000 vectors of 2 KB is
20 MB), fine for a prompt hook's small recall set but likely too slow per keystroke. Step 1
measures a warm search in release; if it misses the budget, the workspace keeps vectors in memory
(checked by the same text hash as the stored ones). Measured (release, 10,000 nodes): keywords only 1.1 ms per keystroke;
with the model 29 ms, then **4.3 ms** once the workspace keeps the vectors (20 MB in memory),
valid while the store's log mark stays put, so any write by any process refreshes them. The first
search, which loads the model and embeds all 10,000 nodes, takes 0.29 s once.

**Behaviour**

- `Ctrl+K` anywhere, inputs included (it is never text), opens a modal near the top: a query
  field and results. `↑`/`↓` (and `Ctrl+N`/`Ctrl+P`) move, `Enter` runs, `Esc` or a click outside
  closes; the mouse hovers and clicks. Typing never runs a command.
- Results in two sections:
  - **Actions**, offered only when legal: on the selected node (acknowledge, ratify, accept,
    reject, move to a state, set priority, add a note, edit the title), navigation (each view,
    each project), the view (list or board, grouping, ordering, show dismissed), and new task.
    Matched by fuzzy subsequence on the label; each shows its shortcut.
  - **Nodes** of the current project: state mark, key, title, kind. An exact key (`LIV-12`,
    `12`) comes first; then the core's search, with the word being typed as a prefix (`resi`
    finds "resize"), fused with meaning when the model is installed.
- An empty query shows the actions for the selected node, then the nodes updated last.
- Choosing a node opens its view, selects it and opens the detail panel, showing dismissed work
  if it is dismissed.

**Core.** `Workspace::find(query, limit)` for typing: a key match, then ranked search with the
last word as a prefix (FTS5 `"word"*`). The current project only; across projects is milestone 6.

**App.** The palette's state, the action catalogue and the fuzzy matcher live in the GPU-free
model, tested in plain Rust; the modal is drawn in `ui`, as the new-task modal is.

**Budget.** Results for a keystroke under 16 ms (one frame) warm on 10,000 nodes. The model loads
once (80 ms), when the palette first opens.

**Steps**, each its own commit:

1. Measure; `Workspace::find` with key and prefix matching; core tests; an ignored benchmark with
   the budget.
2. The palette in the model: state, actions, fuzzy matching; model tests.
3. The modal: keys, rendering; headless UI tests (`Ctrl+K` opens it, typing filters and runs
   nothing, `Enter` opens a node and runs an action, `Esc` closes).
4. README, this plan's status, install.

**Done when** from anywhere, `Ctrl+K`, a few letters and `Enter` reach any node or run any legal
action, and the budget holds on 10,000 nodes.

**What building it found.** GPUI Component's input binds the arrow keys in its own key context,
the deepest, so the palette binds Up and Down under `LivtkPalette > Input`, after the component's
bindings: at equal depth the later binding wins. Kept vectors cost 20 MB at 10,000 nodes. A new
lesson is proposed, so it waits in the Inbox, and the palette opens it there rather than
switching views. Opening the palette prepares search on the UI thread: about 0.3 s once in a
10,000-node project that was never embedded, nothing after.

## 19. Milestone 5, part 2 in detail: the activity feed

**Goal.** See what happened while you were away: every change in the project, newest first,
with what is new since you last looked marked, and the node one key away.

**Behaviour**

- An **Activity** view under Inbox (`g a`): one line per event, grouped by day. Who (you, Claude,
  or which agent), what (created, moved to a state, renamed, edited the plan, set priority,
  noted, linked), the node's key and title, and when.
- **New since you last looked:** events by anyone but you, after the mark, carry an accent dot,
  and the sidebar counts them. Opening the view moves the mark to the newest event. The mark
  lives in the store's `meta` table, which a rebuild keeps.
- `j`/`k` move, `Enter` opens the node in the detail panel without leaving the feed, and the
  panel's actions act on it there. A click does the same.
- Changes from Claude appear at the top while the feed is open, and flash once.
- 200 events a page, newest first; "Show older" loads the next page.

**Core.** `Workspace::activity(before, limit)`: events newest first with the nodes they are
about; `activity_seen`, `set_activity_seen` and `unseen_activity` for the mark.

**App.** The feed's rows, the mark, and acting on the open node live in the model. Actions that
acted on the selected row now act on the node open in the panel when no row is selected, which is
what the feed needs.

**Budget.** A page of 200 under 10 ms on a log of 100,000 events; counting what is unseen under
1 ms, as it runs on every refresh. Measured (release): the newest page 0.4 ms, a page 50,000 deep 1.9 ms. Counting
everything unseen scanned the whole log, 7.5 ms with no mark yet; the count is for a badge, so it
stops at 100 ("99+"): 11 µs.

**Steps**, each its own commit: the core with tests and a benchmark; the model with tests; the
view with headless UI tests; README, plan and install.

**Done when** after a session of Claude's work, the sidebar says how much happened, the feed
shows it newest first, and any line opens its node.

**What building it found.** Actions in the app acted on the selected row, and the feed has no
rows; they now act on the node open in the panel when no row is selected, which also lets the
palette act from the feed. When the human acts from the feed, their event lands on top and the
highlight stays on the event it was on, one line lower.

## 18. Milestone 6 in outline: the team

Decided (developer, 2026-10-16): after milestone 5. livtk becomes a team tool, Linear tailored to
the team's needs and Claude's agentic flow. Outline only; detailed when it starts.

- **Keys that never change.** Today two people who both create `LIV-13` on different branches get
  `LIV-13` and `LIV-14` after the merge, deterministically, but one of them sees their key change
  under them, and every mention of it (commits, chat, notes, bodies) then points at someone
  else's task. Order is not at risk (views sort by priority and time, never by number); identity
  is. As in Linear, the server assigns numbers: a node made offline carries a provisional label
  until the server numbers it, the numbering is itself an event, and a key never changes after.
- **Who did what.** Each human acts under their own name (from git or the livtk config), so the
  log shows who acknowledged a plan or ratified a rule.
- **Hosted.** A server process (`livtk serve`) runs the core against the project and serves the
  app. A spike decides between GPUI's experimental web target, reusing this UI, and a separate
  web front end. Claude Code keeps running on each developer's machine; how its events reach the
  server (through git, or sent directly) is decided then.
- **Sign-in**, so only the team reaches it, and the human gates know which human.
