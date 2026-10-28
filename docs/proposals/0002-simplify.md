# Proposal 0002: simplify livtk

| | |
|---|---|
| **Status** | Draft, for the human's review |
| **Written** | 2026-10-28, by Claude, from the human's brief of the same day |
| **Replaces** | Proposal 0001 (the livtk standard), and the vision half of `docs/plan.md` |
| **Asks for** | A verdict on each item in §4, the decisions in §5–§8, and the order in §10 |
| **Creates no tasks** | Tasks come from this once it is approved |

## 1. The brief, in the human's words

> The product procedurally becomes tech debt, unnecessary flows, code parts, scripts, bad
> architecture overall, the memory layout is not even right for this product — it won't scale in
> numbers, it also won't scale features. The product itself is becoming more like a sandbox of
> putting everything into the product and never asking why things matter to be there, or
> respecting the modern world standards, following latest cutting-edge techs, things that are well
> proven, and no debt for the system architecture, or the design of it. […] This thing is
> open-source and is not at all digestible. […] The system is not automated: the agents should
> learn by themselves, the system should be autonomous, clean itself, reorganize knowledge, merge,
> or prioritize in a system-thinking way.
>
> No user has this software yet. Storage change is not a problem and needs no backfill or
> fallback: previous things are cut off. Simplify first.

## 2. What went wrong (Claude's account)

livtk grew one request at a time, each a patch onto whatever was there, without asking whether it
should exist or where it belongs. The process (plan → ack → check → review) verified that each
change did what it said; nothing verified that it should be in the product, or fitted an
architecture, because none was written down. Research was the exception, not the rule. The result,
measured on 2026-10-28:

- **The app is two god objects.** `LivtkApp` (`ui/mod.rs`, 4,104 lines, ~90 fields, ~385
  methods across `ui/*.rs`) and `Model` (`model.rs`, 2,926 lines, ~140 methods, ~35 fields). Every
  `ui/*.rs` file is `impl LivtkApp`; views are not components. A contributor can't tell who owns
  what, and every feature touches both.
- **Core has a god API.** `Workspace` (2,379 lines) holds config, commands and every query;
  `run.rs` (1,524) mixes process spawning, parsing, worktrees, a queue and locks.
- **The same thing is written several times.**
  - How an event reads in words: five places (CLI output, app activity, timeline kinds, timeline
    model, core signals), plus the hooks.
  - Two shell-line parsers (`run.rs`, `guard.rs`). Two byte-identical `link_row` builders.
  - Six ways to wait: file events, a 3 s `claude agents` poll, a 6 h update loop, a 200 ms
    `events --follow` poll, two 20 ms lock loops.
  - Three sources of "sessions", three ways to tell the human things, two `[[on]]` dispatchers.
  - "Flow" names two different things.
- **Sandbox features.** A terminal (~1.4k lines + alacritty), a year heatmap (~1.1k lines of
  generic "contour" infrastructure for one chart), an endlessly animated state diagram, three
  detail modes, a Markdown editor with Mermaid, 13 tree-sitter grammars, declared kinds and
  relations no project uses, an update pipeline that serves a local folder (80 staging folders and
  224 artifacts, never pruned).
- **Storage was grown, not designed.** See §6. One log that grows forever in one file, a
  projection that stores a second copy of the whole log, keys that silently change meaning after a
  merge, run worktrees and streams inside the project folder.
- **Docs are stale and scattered.** `plan.md` (980 lines) is organized by milestone history, last
  touched 2026-10-19, and contradicts the product (its non-goals rule out what we since built).
  Decisions live only in the event log; the repo can't explain itself.
- **Memory is a list, not a system.** Nothing merges, ages, re-ranks or retires; everything waits
  on the human.

## 3. What livtk is

**One sentence.** livtk is where a human and AI agents build a product together: the work, its
gates, its history and what was learned live in the repository, and the human only decides what is
truly theirs to decide.

**What makes it worth existing** (the survey in §11 found these in no other tool together):

1. **A human gate the agent cannot forge.** Plans need the human's acknowledgement; hooks refuse
   an agent's `ack`. Backlog.md, Spec Kit and Kiro gate by convention only; Beads and Claude Code
   Tasks don't gate at all.
2. **Memory that learns, under the human's authority.** Lessons, decisions and findings, recalled
   into each prompt. Beads' `remember` and Claude Code's auto-memory accept whatever is written.
3. **Verdicts on the work.** An independent reviewer's verdict is recorded on the task.
4. **The history in the repo.** An append-only log in git: who (human, agent, session) did what
   and why, beyond what git's own history says.
5. **Silent unless used.** Nothing happens in a project without `.livtk/`; it composes with the
   project's own hooks.

**What it is not.** Not a terminal, an editor, a markdown IDE, a chart gallery, an agent runner
competing with Claude Code, or a project-management suite (cycles, initiatives, roadmaps). Where
Claude Code or git already does the job, livtk leans on it.

**Who it's for.** People building software with AI coding agents (Claude Code first), alone or in
a small team, who want the agents to do most of the work and want to stay in charge of it.

**Reversal to record.** `plan.md` listed "autonomous self-modification" as a non-goal. The human's
direction reverses it: the system curates its own memory and work, within the risk tiers of §8.

## 4. Every feature, with a verdict

Recommendations; the human decides each. **Keep** = stays, rebuilt on the new architecture.
**Cut** = deleted. **Lean** = delete ours, use Claude Code or git. **Decide** = the human's call,
with Claude's recommendation.

| Feature | Lines (approx.) | Verdict | Why |
|---|---|---|---|
| Tasks: kinds, states, gates, ack | core | **Keep** | The heart (§3.1) |
| Links: depends_on, parent, relates, supersedes, learned_from | core | **Keep** | "Ready" = nothing blocks it: proven everywhere |
| Lessons, decisions, findings + recall | core, CLI, hooks | **Keep**, redesigned (§8) | The heart (§3.2) |
| Projects (a parent above tasks) | 400 | **Keep**, as a node with `parent` links, one level | In use (PRJ-2…5); no cycles, initiatives |
| Inbox: what waits on the human | app, CLI | **Keep** | The human's one queue |
| List and board | 670 | **Keep** | |
| Task panel (detail, timeline, props, jumps) | 2.8k | **Keep**, one mode (not three) | Three modes is three times the surface |
| Command palette, search | 1.2k | **Keep** | |
| New-node form | 1.3k | **Keep** | Just rebuilt on parts (DEC-4) |
| Reviewer verdicts, refuter agent | plugin | **Keep** | §3.3 |
| Plugin: skills, agents, hooks | plugin + 1.1k CLI | **Keep**, trimmed | The integration surface |
| Runs: headless Claude per task, worktrees, queue | 1.5k core + 1k app | **Decide** — recommend **Keep the link, lean on Claude Code for the running** | Claude Code now has subagents and worktree hooks; Conductor/Claude Squad do orchestration; Vibe Kanban, the leader, was dropped by its company. livtk keeps "this task has this run, this branch, this verdict" |
| Sessions view | 1k | **Lean** — keep "sessions that touched this task" in the panel; drop the view | Claude Code owns sessions; one source of truth |
| Agents view + agent-file editor | 1.8k | **Cut** the editor, **Lean** on `.claude/agents` | Editing agent files is Claude Code's job |
| Docs view: reader, editor, find, Mermaid | 3.8k | **Decide** — recommend **keep reading docs linked to a task; cut editing and Mermaid** | Editing Markdown is an editor's job |
| Graph view | 1k | **Decide** — recommend **cut for now** | No measured use; costly to keep right |
| Activity feed | 560 | **Keep**, as the one place "what happened" is read | |
| Year heatmap (activity/contour grid) | 1.1k | **Cut** | Decoration |
| Animated state diagram | 1k | **Cut**; a static state line in the panel | DEC-6's cause |
| Terminal | 1.4k + alacritty | **Cut** | The user's terminal exists |
| Split panes | 840 | **Decide** — recommend **cut** | One list + one panel is the grammar (design.md) |
| Three detail modes, reading sizes, dock | 600 | **Cut** to one | |
| Toasts + top-bar notice + OS notifications | 600 | **Keep one in-window notice + OS notifications** | Three ways is two too many |
| Settings | 1k | **Keep**, only what a user changes; config file for the rest | 2 prefs switches today |
| Declared kinds and relations, `View::Of` | ~600 | **Cut** | No project uses them; YAGNI |
| `[[on]]` command hooks, signals, `events --follow` | 900 | **Decide** — recommend **keep `events` as one stream, cut `[[on]]`** | Two dispatchers; Claude Code hooks cover most |
| Export to markdown | 150 | **Cut** | `show` and the log are enough |
| Flow metrics (`livtk flow`) | 180 | **Keep**, renamed (`livtk lead-time`) | The other "flow" goes |
| Update pipeline | 1k + 2 scripts | **Keep, rebuilt** — releases from GitHub, pruned staging | The human's own loop takes builds through it; open source needs a real channel |
| Deprecated config still parsed, `ClaimSession` | small | **Cut** | No users to migrate |
| `rows.rs` (avoid an 80 ms model load) | 680 | **Decide** after §8 — recall may move out of the hook | |
| Test-only fields in `LivtkApp` | small | **Cut** | Tests drive public seams |
| Scripts: check, energy, scale, release, bundle-app, watch | 580 + 2 new | **Consolidate into `cargo xtask`** | One tool, discoverable, cross-platform |
| `prototypes/`, `docs/prototypes/`, `screenshot.png`, `.DS_Store`s | — | **Cut** | Cruft |

If every recommendation is taken, the app loses roughly 12–14k of its 32k lines before the
rebuild starts.

## 5. Target architecture

Modelled on jj (library vs CLI), helix (domain / state / frontend) and Zed (state in store
entities, views per concern, stateless components). Sources in §11.

### 5.1 Crates

```
crates/
  livtk-core      domain: ids, kinds, states, transitions, events, invariants. Pure, no I/O.
  livtk-store     the log (files) and the projection (SQLite): append, replay, query.
  livtk-service   commands and queries the CLI, hooks and app all call: create, move, ack,
                  recall, curate, link a run. The one place a rule is enforced.
  livtk-cli       clap and output only; hooks are CLI commands.
  livtk-ui        design-system parts: stateless RenderOnce components, theme tokens. No domain.
  livtk-app       stores as entities, views, wiring.
  livtk-update    release channel (GitHub), staged install.
xtask             check, release, bundle, energy, scale, fixture, icons.
```

Rules (written into `ARCHITECTURE.md` as **absences**, per matklad):

- core does no I/O and knows no SQLite, no GPUI, no clap;
- only `livtk-service` writes events; the CLI and the app never touch the log or the store;
- `livtk-ui` knows nothing of tasks;
- a view never queries the store; it reads its store entity (DEC-7);
- a trait exists only where a second implementation does: the log sink (file / memory), the
  clock and ids, the agent runner (Claude / fake).

### 5.2 The app

- **Stores as entities**, one per concern: `Tasks`, `Memory`, `Runs`, `Inbox`, `Activity`,
  `Settings`. Each wraps its slice of the projection, refreshes on the log's file events, and emits
  typed events (`TasksEvent::Changed(Key)`). The watcher feeds stores, not views.
- **Views as entities**, one per section (Inbox, Tasks, Memory, Activity, Settings) and one panel
  (`TaskPanel`). Each owns its own scroll, selection and form state and subscribes only to the
  stores it shows. A `Section` trait (title, icon, render, focus, save) — like Zed's `Item` and
  `Panel` — so adding a view is adding a type, not editing a match.
- **`LivtkApp` becomes a thin workspace**: the sidebar, the sections, the panel. Commands are GPUI
  Actions; keys live in one keymap.
- **Parts** are `livtk-ui` components only; nothing hand-rolled in views (LES-11).
- **One event-wording module** in core (`describe(&Event) -> Line`), used by the CLI, the
  activity feed, the timeline, signals and hooks.

Tests follow: each view tested alone with a fake store; the store tested against the service; the
one app test binary stays (matklad), smaller because views are small.

### 5.3 Size rules

A module over ~1k lines or holding two concepts is split; a type with more than ~30 methods is a
smell, reviewed. The refuter checks both.

## 6. Storage, designed for 200,000 events and for merges

No compatibility: the new layout replaces the old, and this repository's own log is converted
once by a throwaway script.

### 6.1 What it is today (measured, 2k-event fixture, release build, 2026-10-28)

| | |
|---|---|
| Log | 1,347 KB; gzip 368 KB; zstd -19 295 KB |
| Projection | 2,944 KB, of which the `events` table (a full copy of every log line) is the largest part |
| Cold build of the projection / rebuild | 77 ms / 57 ms |
| `list`, `show`, `log`, `inbox`, `next` | 11–14 ms; `sessions` 23 ms |
| Prompt hook (recall) | 113 ms |
| A note (append + apply) | 26 ms |
| git: first commit (gc'd) / after 100 notes | 492 KB / 496 KB |
| App: memory after 20 s / CPU to start | 113 MB / 0.44 s |

The 20k and 200k measurements were lost when the machine shut down; they are re-run, one size at
a time and on mains power, as the first step of the storage work. The real log (2,050 events)
grows by ~1,000 events a week of active use.

### 6.2 Problems found

1. **Keys change meaning after a merge.** Two branches each create `TAS-12`; after the merge the
   earlier keeps it and the other silently becomes `TAS-13`. A key written in a commit message
   now names another task, and nothing tells anyone. (Reproduced on 2026-10-28.)
2. **One file forever.** Every commit of a 140 MB log (200k events) writes a ~38 MB loose object
   until `git gc` (FND-12); hashing it on every `git add`; blame grows with history.
3. **The projection copies the log.** `events.line` stores each JSON line again; 13 hot queries
   filter with `json_extract`, which no index serves.
4. **Runs and worktrees live in the project folder.** 17 MB of streams, never pruned; whole
   checkouts with their own `target/` (6–7 GB, FND-5) under `.livtk/runs/worktrees`, seen by the
   IDE, Spotlight, Time Machine and our own file watcher.
5. **User state mixes kinds.** `app.toml` holds preferences, window state and recent paths
   (including test folders); no atomic writes.
6. **WAL unbounded.** No `journal_size_limit`.

### 6.3 The design

| What | Where | Format | Why |
|---|---|---|---|
| **The log** (truth, shared) | `.livtk/log/YYYY-MM.jsonl`, committed | JSON lines, plain | Monthly segments: a commit writes only the open month; closed months never change; diffs and blame stay local (Kafka-style segments). Plain text: git compresses and deltas it; compressing twice gains nothing in the repo |
| **Ids and keys** | in each event | uuid v7 + **key = prefix + short hash** (`TAS-k3f9`) | Collision-free across branches, as Beads does; no counters, no renumbering. *Alternative for the human:* keep numbers and show the change loudly after a merge — rejected as a recurring hazard |
| **Replay order** | — | by event id (time-ordered) | Deterministic after a union merge, whatever the line order |
| **Merge** | `.gitattributes` | `merge=union` per segment | Safe: lines are self-contained, ids unique, replay by id |
| **The projection** | `~/Library/Caches/livtk/<project-id>/index.db` (XDG: `$XDG_CACHE_HOME`) | SQLite, WAL, `synchronous=NORMAL`, `journal_size_limit` | Rebuildable, so it is a cache: out of the repo, the IDE, Spotlight and backups |
| **Projection schema** | — | typed columns for what queries filter (state, run, health); an offset into the log instead of the line | No second copy; every hot query indexed; `EXPLAIN QUERY PLAN` asserted in tests ("no SCAN") |
| **Run streams** | `~/Library/Caches/livtk/<project-id>/runs/` | JSONL while live; **zstd once closed** (~9× smaller) | Local, derived; retention: keep the last N or M days (configurable) |
| **Run worktrees** | `~/Library/Caches/livtk/<project-id>/worktrees/<key>` | git worktree | Out of the project folder; pruned when the run ends |
| **Preferences** | `~/.config/livtk/settings.toml` | TOML | What a person sets |
| **Window state, recents** | `~/Library/Application Support/livtk/state.toml` (XDG: `STATE_HOME`) | TOML, atomic write (temp + rename) | What the app remembers; test paths never recorded |
| **Project list** | same state folder | TOML | Machine-specific |
| **Models** | `~/Library/Caches/livtk/models` | as today | Rebuildable |

A body edit keeps storing the whole body (git deltas it; a diff format adds complexity for little).

**Considered and rejected:** Dolt (Beads' move: a version-controlled SQL database) — a heavy
dependency, not plain text, not reviewable in a pull request. A binary log (MessagePack/CBOR) —
no line diffs, no union merge, gains mostly erased by git's zlib.

## 7. The workflow that keeps it simple

- **Why before what.** Every plan starts with one line: *which part of §3 does this serve?* A plan
  that can't say is a proposal, not a task.
- **Research before design.** A plan for anything new cites what exists (a quick researcher run):
  the closest tools, the proven practice, the primary sources. Memory alone is not a source.
- **Size of the decision decides its form.** A line in a plan → a task; a choice between options →
  a decision record (`docs/decisions/`, MADR); a change of direction → a proposal like this one.
- **Delete as you replace.** A change that replaces something removes the old in the same task.
- **Two paths.** A small fix: the tests it touches, a commit, done. A feature: plan, ack, build,
  the full check, the reviewer. The full check, a release and the reviewer run per task, not per
  tweak.
- **Measured claims only.** A number in a note is re-measured the way the claim states it.
- **The machine is not a build server.** One heavy job at a time; scale and energy runs on mains
  power.

## 8. Memory and work that curate themselves

From Letta (sleep-time agents), Mem0 (update ops), Zep/Graphiti (invalidate, never delete),
A-MEM (linking), Generative Agents (scoring), Anthropic's Dreams and context-engineering guidance,
and Claude Code's own memory caps. Sources in §11.

**On write** (immediate, no model call): the new node is embedded and compared with its
neighbours. Very close to one of its kind → marked a duplicate candidate; close → linked
(`relates`). Each node carries `source` (human, agent, session, tool), `valid_from` and, later,
`importance` and `use`.

**On use**: every recall is logged (prompt hash, nodes). At session end a judge notes whether each
recalled node was followed, ignored or contradicted by the human's correction. That is `use`.

**Sleep pass** (when idle, or after N new events; one curator agent): reads what changed and
recent transcripts, and proposes a changeset of operations, each with its reason, its sources and
a reversible diff:

- `MERGE` near-duplicates; `SUPERSEDE` (invalidate, never delete) the contradicted;
- `PROMOTE` a finding seen again and again to a lesson candidate;
- `REFLECT`: a new finding that summarises a pattern;
- `ARCHIVE` what is never recalled; re-rank (relevance + recency + importance + use).

It also tends the work: duplicate tasks, stale ones, what blocks what, and the priority of what's
ready — proposed the same way.

**Who decides** (by the harm a change can do and whether it can be undone, never by the model's
confidence alone):

| Tier | Applies | Examples |
|---|---|---|
| Automatic, logged, undoable | at once | merge findings (only if no number, path or version differs, and a second judge agrees); archive findings unused for 60 days; re-rank; add links |
| Automatic after a veto window | after 72 h unless the human objects | promote a finding to a decision of record when it agrees with accepted ones |
| Always the human | never automatic | create or change a lesson (a standing rule); supersede anything the human wrote, ratified or accepted; contradictions between ratified nodes; anything about secrets, deletion or security; anything that came from web or tool output |

Every tier starts in "propose". A tier becomes automatic only while the human's measured
agreement with it, on a sample, stays at 95% or above.

**The human's view**: one *Memory* digest — the always-human queue, and a daily log of what was
done automatically, each with an undo.

**Bounds**: active lessons ≤ ~40, active findings ≤ ~500, recall ≤ 3 per prompt; over a bound, the
sleep pass archives the lowest-ranked first. Archived and superseded nodes stay searchable but are
never injected.

**Measured**: a replay set of real prompts and the nodes they should recall (precision@3,
staleness), and recall on/off on the same tasks (corrections per task).

**Where it lands in Claude Code**: ratified lessons are written as a generated
`.claude/rules/livtk.md`, so Claude Code loads them by its own rules; per-prompt recall stays for
decisions and findings. (To confirm in research: whether this replaces the prompt hook's recall.)

All thresholds above are starting guesses, tuned on the replay set.

## 9. Docs that explain the project

```
README.md  ARCHITECTURE.md  CONTRIBUTING.md  CHANGELOG.md  SECURITY.md
docs/
  tutorial/      getting-started.md            (install → first task → first run → first lesson)
  how-to/        add-a-view.md, add-a-command.md, release.md, measure-energy.md
  reference/     cli.md (generated from clap), config.md, events.md, design-system.md (← design.md)
  explanation/   log-and-projection.md, gates.md, memory.md
  decisions/     NNNN-title.md (MADR; DEC-n exported here; superseded, never rewritten)
  proposals/     NNNN-title.md (this one; accepted ones spawn decisions)
  research/      dated, labelled non-normative
```

`plan.md` is retired: its still-true parts go to `ARCHITECTURE.md` and `explanation/`, its history
to git. Proposal 0001 is withdrawn in favour of this one.

## 10. The order of work

The human's call (2026-10-28): **the experience and the UI come first.** The UI framework is the
boundary that shapes the rest: views that read typed stores need a service behind them, so the
backend is pulled into shape by the front, and both scale together.

Each step leaves the product working and is its own small set of tasks. No new features meanwhile.

1. **Decide.** The human marks §4's verdicts; each becomes a decision record.
2. **The UI framework** (`livtk-ui`): design tokens, themes (light, dark, system: TAS-126),
   typography, spacing, motion, focus and keys, the components, and a gallery to see and test them
   alone. Then the app shell on it: the thin workspace, the `Section` trait, stores as entities.
   Planned in its own proposal (0003), from research.
3. **Cut** what §4 cuts, as the shell replaces the old app: nothing is ported that is cut.
4. **Views, one by one**, on the shell and the framework, each with its store; the old code deleted
   as each lands. Tests per view.
5. **Storage** (§6) and the **core split** (§5.1), as the stores need them: measured at 2k, 20k
   and 200k before and after.
6. **Memory and work curation** (§8), every tier starting in "propose".
7. **Docs** (§9) alongside: `ARCHITECTURE.md` with step 2, the rest as each part settles.

## 11. Sources

**Comparable tools.**
- Beads: <https://github.com/steveyegge/beads>
- Backlog.md: <https://github.com/MrLesk/Backlog.md>
- Task Master: <https://github.com/eyaltoledano/claude-task-master>
- GitHub Spec Kit: <https://github.com/github/spec-kit>
- Kiro: <https://kiro.dev/docs/specs/>
- Vibe Kanban: <https://www.vibekanban.com/blog>
- Claude Squad: <https://github.com/smtg-ai/claude-squad>
- git-bug: <https://github.com/git-bug/git-bug>
- Linear: <https://linear.app/docs/conceptual-model>
- Plane: <https://github.com/makeplane/plane>
- Claude Code hooks: <https://code.claude.com/docs/en/hooks>
- Claude Code memory: <https://code.claude.com/docs/en/memory>

**Architecture.**
- Zed, GPUI ownership: <https://zed.dev/blog/gpui-ownership>
- Zed glossary: <https://github.com/zed-industries/zed/blob/main/docs/src/development/glossary.md>
- jj architecture: <https://github.com/jj-vcs/jj/blob/main/docs/technical/architecture.md>
- matklad, ARCHITECTURE.md: <https://matklad.github.io/2021/02/06/ARCHITECTURE.md.html>
- matklad, one test binary: <https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html>
- Rust API Guidelines: <https://rust-lang.github.io/api-guidelines/>
- Diátaxis: <https://diataxis.fr/>
- MADR: <https://adr.github.io/madr/>

**Storage.**
- git gitattributes (union): <https://git-scm.com/docs/gitattributes>
- SQLite WAL: <https://www.sqlite.org/wal.html>
- SQLite FTS5: <https://www.sqlite.org/fts5.html>
- SQLite query plans: <https://www.sqlite.org/eqp.html>
- zstd: <https://github.com/facebook/zstd>
- git-appraise: <https://github.com/google/git-appraise>
- Apple, File System Programming Guide (Caches vs Application Support)
- FND-12

**Memory.**
- Letta sleep-time compute: <https://www.letta.com/blog/sleep-time-compute/> and <https://arxiv.org/pdf/2504.13171>
- Mem0: <https://arxiv.org/pdf/2504.19413>
- Zep/Graphiti: <https://arxiv.org/pdf/2501.13956>
- A-MEM: <https://arxiv.org/abs/2502.12110>
- Generative Agents: <https://arxiv.org/abs/2304.03442>
- Anthropic Dreams: <https://platform.claude.com/docs/en/managed-agents/dreams>
- Anthropic, context engineering: <https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents>
- Trust or Escalate (ICLR 2025)
- Memory poisoning (MINJA): <https://arxiv.org/html/2503.03704>

Not verified, flagged in the research: Beads' and Task Master's star counts, Claude Code's
"Auto Dream", Cursor's removal of Memories.

## 12. Questions for the human

1. §4's **Decide** rows: runs, the docs view, the graph, split panes, `[[on]]` hooks, `rows.rs`.
2. Keys as short hashes (`TAS-k3f9`) instead of numbers (§6.3): acceptable?
3. Should ratified lessons be delivered as Claude Code rules files (§8), in addition to or instead
   of the prompt hook?
4. Is Claude Code the only agent livtk targets for now, or should the design keep others (Codex,
   Cursor) in view?
