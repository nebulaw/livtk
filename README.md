# livtk

![The livtk app: the graph of a project's work, with a task open in the detail panel](screenshot.png)

Durable tasks and memory for Claude Code and its human. Local only: no API, no server, no keys.

Claude plans work and moves it along; the human acknowledges each plan before it starts. Every
change is an event in `.livtk/events.jsonl`, committed with the project, so history survives
sessions, crashes and git branches. A SQLite database next to it answers queries fast and can
always be rebuilt from the log.

The design is in [docs/plan.md](docs/plan.md).

## Install

The `livtk` command, then the Claude Code plugin (hooks, skills, agents) from this repository:

```
cargo install --locked --path crates/livtk-cli      # the livtk command
claude plugin marketplace add /path/to/this/repo     # or: /plugin marketplace add nebulaw/tasks
claude plugin install livtk@livtk                    # or: /plugin install livtk --marketplace nebulaw/tasks
```

The app is **Liv** (`crates/liv`), being built from scratch (proposal 0003, PRJ-6). On macOS:

```
cargo install --locked cargo-bundle    # once
cargo xtask liv --install              # Liv.app in ~/Applications
```

### Updates

Liv looks for a newer build when it starts, every 6 hours, and from Liv ▸ Check for Updates….
Every build it takes is signed (minisign), and it trusts only the keys built into it. It
downloads and checks a build in the background, offers **Restart to Update**, and never updates on
its own; the new build finishes the swap once Liv has quit, and the old one comes back if the new
one doesn't start. Stable and preview builds come from this repository's GitHub releases (TAS-140);
builds made on this machine come from Liv's local channel:

```
cargo xtask liv --publish "what changed"   # commit first: a build is numbered by its commit
```

The plugin is silent in any project without a `.livtk/` directory, so it can stay enabled
everywhere. Its hooks only do what livtk alone knows: brief Claude at session start and before a
prompt, guard the human's commands and `.livtk/`, and refuse the implementer's stop while a task
in progress has nothing recorded about it. **Formatting and build checks are not livtk's:** run
your formatter from your own `PostToolUse` hook and your check from your own `SubagentStop` or
`Stop` hook (see Claude Code's hooks documentation). A `[hooks] format` or `check` left in
`.livtk/config.toml` is ignored with a warning, and becomes an error in the next release.

**A project with hooks of its own.** Claude Code runs every hook registered for an event, so a
project that already briefs Claude at session start gets two overlapping blocks. Two ways out:

- Keep the plugin, and tell livtk what to leave out, in `.livtk/config.toml`:

  ```toml
  [hooks]
  disable = ["session-start"]          # or user-prompt-submit, subagent-start, subagent-stop

  [session_start]
  include = ["tasks", "gates"]         # of intro, tasks, projects, gates, protocol, rules

  [user_prompt_submit]
  include = ["memory"]                 # of memory (what is recalled), nudge (propose a lesson)
  ```

  Nothing set, everything is said, as before. A name livtk does not know stops it with an error
  naming it, so a typo never switches something off quietly. The guard on the human's commands
  cannot be switched off.

- Or skip the plugin and call livtk from your own hook: `livtk hook <event>` reads the hook's
  JSON on stdin and answers in Claude Code's own shape, so one hook per event says both:

  ```bash
  #!/usr/bin/env bash
  # .claude/hooks/session-start.sh: your project's own context, then livtk's, as one block.
  input=$(cat)
  ours="Branch: $(git branch --show-current 2>/dev/null)"
  theirs=$(printf '%s' "$input" | livtk hook session-start | jq -r '.hookSpecificOutput.additionalContext // empty')
  jq -n --arg c "$ours"$'\n'"$theirs" \
    '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $c}}'
  ```

**A project's own kinds and relations.** livtk ships five kinds (task, lesson, decision,
finding, project) and five relations (waits for, relates, supersedes, learned from, part of). A project
that needs its own words declares them in `.livtk/config.toml`; one that declares nothing gets
exactly the built-in set.

```toml
[[kinds]]
name   = "incident"
states = ["open", "mitigated", "closed"]   # in order; it starts at the first unless `start`
closed = ["closed"]                        # what counts as over (the last state unless given)
human  = ["mitigated -> closed"]           # moves only you may make
key    = "INC"                             # its keys: INC-1, INC-2
memory = false                             # true: recalled like lessons and findings
icon   = "bug"                             # its icon in the app; a name it lacks is said, with the names it has

[[relations]]
name    = "implements"
inverse = "implemented_by"                 # how it reads from the other end
```

A declared kind moves between any two of its states, is created with `livtk new --kind incident`,
listed with `livtk list --kind incident`, gated by `[gates]` like any other, found by search, and
has its own view in the app with its states as groups and board columns. A declared relation is
linked with `livtk link A implements B` (or `livtk link B implemented_by A`) and shows from both
ends. A kind cannot be taken out of the config while nodes of it exist: livtk stops and names
them. A name livtk already uses is refused.

Other optional per-project settings in `.livtk/config.toml`:

```toml
[gates]
human = ["task: review -> done", "task: -> canceled"]  # more moves only you may make
```

Acknowledging a plan and deciding what binds (ratifying, accepting, rejecting, retiring) are
always yours. `[gates]` can only add to them: the file is in the repository, where Claude could
change it, so an edit to it can make Claude stricter, never freer. An entry is `kind: from ->
to`, or `kind: -> to` from any state; one that names no legal move stops livtk with an error.
What waits on a gate from a state shows in your inbox, and the app marks such moves as your
decisions.

## Use

```
livtk init                          # once per project: creates .livtk/, commit it
livtk new "Draw the first window" -p 2
livtk new "Simple compositor" --after LIV-1 --body - < plan.md
livtk new "Damage tracking" --parent LIV-2   # a sub-task
livtk link 3 relates 1              # also waits-for, blocks, learned-from, supersedes,
                                    # part-of, has-part; `livtk unlink` takes the same words
livtk list                          # open tasks by state (aliases: ls, board)
livtk move 1 planned                # Claude proposes the plan
livtk ack 1                         # you acknowledge it (human only)
livtk next                          # the best task to start now
livtk start 1                       # refused while a dependency is open
livtk move 1 review
livtk done 1
livtk note 1 "winit panics on resize; trying the drm path"
livtk show 1                        # links, docs, body, notes, history
livtk log                           # every event in the project
livtk doc list                      # the docs (markdown under docs/), with their links
livtk doc link 1 docs/guide.md      # link a node to a doc; any number each way
livtk doc links 1                   # a node's docs; `livtk doc links docs/guide.md`, a doc's nodes
```

Docs are the project's markdown files, under `docs/` (or the folder `docs = "..."` in
`.livtk/config.toml` names); linked to nodes through the log, by their path.

Keys accept `LIV-12`, `liv-12` or just `12`. Every command takes `--json`.

**Projects: an outcome above tasks.** A project (`PRJ-1`) says what should be true, its body
ending in a `Done when:` line. Claude may propose one; only you commit to it (`livtk move PRJ-1
active`) and call it done. Tasks join it as parts (`livtk new "..." --parent PRJ-1`, or `livtk
link 3 part-of PRJ-1`), and projects nest the same way; a node has one parent, so it counts
toward one project. Its progress is counted, never typed in: tasks done out of all of them
(canceled ones aside), and what waits on you (planned for your ack, in review for your check, and
whatever your gates hold), with the one waiting longest. Its health is written: an update says on track, at
risk or off track, what moved and what is next; an active project a week without one reads
"update missing".

```
livtk project new "Import the board" --body - --target 2026-12-01 --owner karl < outcome.md
livtk project list                  # active first: state, done/total, health, what waits on you
livtk project show PRJ-1            # its outcome, parts, waits and newest update
livtk project add TAS-12 PRJ-1      # put a task in a project (out of the one it was in)
livtk project remove TAS-12         # take it out
livtk project update PRJ-1 --health at_risk "parsing is slow; next: batch it"
livtk project set PRJ-1 --target "" # an empty value clears it
```

**Flow, measured.** `livtk flow` reads the log for how long tasks spend in each state (the
median per task, and all together), what waits on you now and the longest wait, and with
`--project PRJ-1` that project's tasks alone and its lead time, from its first task started to
done. The app shows the waits above the activity feed, and a project's in its panel.

More than three active at once, and `project list` says so: fewer at a time finish sooner. Lists
name a task's project (`[PRJ-1]`, and `"project"` in `--json` rows) once there is one. Project
events are format 6: an older livtk skips them and asks to be updated.

**Which Claude session did what.** Every change Claude makes through livtk names the Claude
Code session it came from, besides the agent: the plugin's hook adds both to each `livtk` command
it runs, so nothing is left to Claude to remember. Your own changes name none. `livtk show KEY`
lists a node's sessions (which agent, how many events, when), and `livtk sessions` lists the
sessions with the nodes each touched. Work done before livtk recorded sessions can be credited to
the session it came from, once: `livtk sessions claim` matches each earlier event to the Claude
Code transcript that ran its command, and gives the rest to `--default <session>` (`--dry-run`
says what it would do). An older livtk reads past the session; a claim is format 7.

**Runs: Claude working on a task on its own.** `livtk run KEY` starts Claude Code headless on a
task, for the stage its state is ready for: planning from the backlog, building once you have
acknowledged it, reviewing in review (`--stage` picks another, `--agent` another agent). A build
runs as `livtk:implementer` and a review as `livtk:refuter`, unless the task was given to another
agent with `livtk agent set KEY NAME` or an agent's file says `stage: build`. A task that waits for
unfinished work is not built until that work is done or canceled; planning or reviewing it
never waits. The run is
written on the task as notes: that it started, what Claude says when a turn ends, and how it
ended (done, failed, stopped). When it needs you, a note says so and the task waits in your
inbox:
- **A question:** answer it with `livtk note KEY "..."`, and the run reads your note next.
- **A command it may not run on its own** (it runs with nothing allowed beyond reading, editing
  for a build, and livtk itself): `livtk run allow KEY` lets it run exactly that, and
  `livtk run deny KEY` refuses it. Both are yours alone.

`livtk runs` lists the runs going or waiting (`--all` the ended too). `livtk run stop KEY` ends
the turn cleanly and keeps the work; `livtk run continue KEY` takes it up again. Each turn is its
own process, writing to `.livtk/runs/` (local, never committed), so runs outlive the terminal or
app that started them; whichever livtk looks next writes what they did. Run in a git worktree,
`livtk` writes to the main checkout's log. Run notes are format 8.

**Runs on their own.** A project's `[runs]` (Settings › Runs) can have them start without
anyone pressing Start: `auto = true` starts building each acknowledged task given to an agent,
`builds_at_once` of them together (2 unless set; the rest wait their turn, by priority, listed
in Agents), and starts the review of each task that comes into review. Off by default. With
`worktree` (on by default, in a git repository), each build works in its own worktree under
`.livtk/runs/worktrees/`, on its own branch `livtk/<key>`, and its review reads it there; when
you mark the task done, the branch comes into your checkout's branch, fast-forward only, and
the worktree goes. One that cannot fast-forward is left for you, with a note saying why. The app
is where you follow all of this; for a setup with no app (an agent in a terminal only),
`scripts/watch` prints the human's moves and notes as they happen.

**For scripts.** `livtk list --json` gives small rows; `--fields key,state,props` picks what each
row carries, a node's own `body`, `props`, `created` and `created_by` included. `livtk show KEY
--json` gives everything about one node: its fields with body and props, its links, its notes and
its history.

**A markdown mirror, if you want one.** `livtk export` writes every node as a markdown file
(fields as frontmatter, then the body, the links and the notes) into `.livtk/export`, which keeps
itself out of git; `--dir` writes elsewhere, and `--check` fails if the folder is behind the log,
for CI. **It is a copy, never read back: editing an exported file changes nothing in livtk.** The
log is the truth, and an export is whatever this machine can read of it, wherever the log lives.

### Signals

Agents and tools hear what happens as signals, one JSON object per line, derived from the log:
`node.created`, `node.moved`, `node.edited`, `node.noted`, `link.added`, `link.removed`,
`doc.linked`, `doc.unlinked`, and the
ones worth waking for: `decision.needed` (something waits for you), `decision.made` (you
acknowledged, ratified, accepted, rejected or retired) and `work.unblocked` (the last thing a task
waited for is done). Each names its event; keep the last one you saw and read on from it:

```
livtk events                                        # every signal so far
livtk events --since <event> --follow               # what comes after, as it comes
livtk events --follow --signal decision.made --node 12
```

Signals are versioned (`"v": 1`): fields are only ever added, and a reader ignores the ones it
does not know. Act on a signal's `node` (its id never changes); its `key` is the node's key as it
stands when read, which a merge can still renumber. `--follow` also prints what a git pull merges
in, older events included.

A project can also run a command on a signal, in `.livtk/config.toml`:

```toml
[[on]]
signal = "decision.made"            # and optionally: kind = "task"
run = ["./scripts/wake-agent"]      # the signal's JSON on stdin, run from the project's folder
```

Hooks run after the change is in the log, apart from whatever made it (the CLI starts them on
their own and returns; the app runs them off its window), so a hook never holds up or undoes a
write. Each hook keeps its place (`.livtk/on-state.json`, local to the machine): a failing one is
tried again on the next write, in order, missing nothing, and an event merged in from elsewhere
within 30 days is heard too; a new one hears what comes, not the project's history. A hook's
command runs with `LIVTK_HOOK` set: `livtk` run inside it acts as `claude:hook` (never as you, so
it passes none of your gates), and no hook hears what a hook did, so hooks never wake each other
in a loop. `livtk dispatch` runs them by hand.

## Memory

Claude proposes what it learns; you decide what binds it.

```
livtk lesson "Run cargo check before reporting a task done" --why "a broken build was reported done"
livtk decision "SQLite for local tools" --body "one file, no daemon"
livtk finding "winit panics on resize under NVIDIA" --body "seen with driver 580"
livtk inbox                          # everything waiting for you
livtk ratify LES-1                   # a rule, recalled when relevant (human only)
livtk ratify LES-1 --always          # a standing rule, shown at every session start
livtk accept DEC-1                   # human only; reject and retire too
livtk recall "is the task done?"     # what a prompt would bring back
livtk search "resize"                # every node, by keywords and meaning
livtk embed setup                    # once: the local model (128 MB) for ranking by meaning
```

Lessons, decisions and findings have their own keys (`LES-1`, `DEC-1`, `FND-1`). Editing a
ratified lesson or an accepted decision sends it back for approval, as with tasks. With the
plugin, each prompt carries at most three relevant rules, decisions or findings, and a prompt
that reads like a correction nudges Claude to propose a lesson. Without the model, search and
recall use keywords only. With it, a prompt costs about 5 ms: livtk reads only the rows of the
model the prompt needs, and keeps a sorted copy of its vocabulary (`livtk-vocab.txt`) beside it.

States: `backlog → planned → acked → doing → review → done`, plus `parked` and `canceled`.
Only a human can acknowledge (`planned → acked`). Editing a task's plan once it is acknowledged
(`acked`, `doing` or `review`) sends it back to `planned`, to be acknowledged again. A command run by Claude Code (which sets `CLAUDECODE=1`) acts as Claude; anything else
acts as the human. Refusals by a rule exit with code 3.

## The old app

The first app, livtk.app, is kept in [legacy/](legacy/) for reading only: nothing builds or tests
it. A copy installed earlier keeps working, and its `livtk-app` and `livtk` links stay where they
were. Liv replaces it (TAS-137).

## Develop

```
scripts/check              # format, lint, supply chain, every test, benchmarks and model tests
cargo xtask liv            # bundle Liv; --install, --publish [notes], --profile release
```

The supply chain step runs [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
(`cargo install --locked cargo-deny`) against `deny.toml`. It allows permissive licences and
MPL-2.0 but no GPL or LGPL, and crates.io as the only source. It fails on vulnerabilities and
yanked versions, and on unmaintained crates livtk depends on directly. The advisory database is
fetched each run; without a network, the cached copy is used.

The CLI's benchmarks time the installed binary, start-up included, so they run in release; the
core's run in the `quick` profile, release speed without LTO, which rebuilds in seconds. The model
tests run when the model is in `~/.cache/livtk/models` (or `LIVTK_TEST_MODELS`).

Builds go to `target/`. Cargo never prunes it and it grows with each toolchain and profile; clear
what isn't needed (the incremental caches first) before reaching for `cargo clean`, which makes
the next build recompile everything, GPUI included.

## Licence

MIT. The app's icons are [Boxicons](https://boxicons.com), under CC BY 4.0: see
[legacy/livtk-app/assets/icons](legacy/livtk-app/assets/icons).
