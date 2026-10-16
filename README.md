# livtk

Durable tasks and memory for Claude Code and its human. Local only: no API, no server, no keys.

Claude plans work and moves it along; the human acknowledges each plan before it starts. Every
change is an event in `.livtk/events.jsonl`, committed with the project, so history survives
sessions, crashes and git branches. A SQLite database next to it answers queries fast and can
always be rebuilt from the log.

The design is in [docs/plan.md](docs/plan.md).

## Install

The binary, then the Claude Code plugin (hooks, skills, agents) from this repository:

```
cargo install --locked --path crates/livtk-cli
claude plugin marketplace add /path/to/this/repo     # or: /plugin marketplace add nebulaw/tasks
claude plugin install livtk@livtk                    # or: /plugin install livtk --marketplace nebulaw/tasks
```

The plugin is silent in any project without a `.livtk/` directory, so it can stay enabled
everywhere. Optional per-project hook settings live in `.livtk/config.toml`:

```toml
[hooks]
format = ["rustfmt", "--edition", "2024", "{file}"]   # after Claude edits a matching file
format_extensions = ["rs"]
check = ["cargo", "check", "--quiet"]                 # before the implementer agent may stop
check_agents = ["livtk:implementer"]
```

## Use

```
livtk init                          # once per project: creates .livtk/, commit it
livtk new "Draw the first window" -p 2
livtk new "Simple compositor" --after LIV-1 --body - < plan.md
livtk list                          # open tasks by state (aliases: ls, board)
livtk move 1 planned                # Claude proposes the plan
livtk ack 1                         # you acknowledge it (human only)
livtk next                          # the best task to start now
livtk start 1                       # refused while a dependency is open
livtk move 1 review
livtk done 1
livtk note 1 "winit panics on resize; trying the drm path"
livtk show 1                        # links, body, notes, history
livtk log                           # every event in the project
```

Keys accept `LIV-12`, `liv-12` or just `12`. Every command takes `--json`.

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
recall use keywords only.

States: `backlog → planned → acked → doing → review → done`, plus `parked` and `canceled`.
Only a human can acknowledge (`planned → acked`); editing an acknowledged plan sends it back to
`planned`. A command run by Claude Code (which sets `CLAUDECODE=1`) acts as Claude; anything else
acts as the human. Refusals by a rule exit with code 3.

## The app

```
cargo install --locked --path crates/livtk-app
livtk-app                    # every project `livtk init` registered
livtk-app ~/code/project     # open one (and remember it)
```

The human's control surface: an Inbox of everything waiting for you, an Activity feed of every
change newest first (its badge counts what Claude did since you last looked), a Graph of every
node and link where work flows left to right along its dependencies, tasks as a list or a board,
lessons, decisions and findings, and a detail panel with the plan, its links and its activity.
Changes from Claude show up while it is open, and flash once. It wears Linear's darkest palette.
As in Linear, the Filter menu (`/`) narrows by title, key or priority, and the Display menu
holds Grouping (state, priority) and Ordering (priority, last update, creation), each its own
menu, and whether dismissed work shows; groups are solid bands that collapse on a click.
Double-click a title or body in the detail panel to edit it: Enter (Ctrl+Enter for the body)
or clicking away saves, Esc cancels. Editing an acknowledged plan sends it back to planned. Drag a row
onto another group, or a card onto another board column, to move it; only legal moves light up.
Done work stays in view; what was dismissed (canceled, rejected, retired) shows with `h`.
A task's priority icon, in a row, on a card or in the detail panel, opens a menu to change it;
a state icon in a row or on a card opens its kind's states, the moves it can make enabled and
your own decisions (acknowledge, ratify, accept, reject) in the accent. The detail panel draws
the states as a flow from left to right: where it is now filled, where it can go outlined and
clickable; ratifying offers the standing rule.
Links are added from the detail panel (+ Add link: waits for, blocks, related to, learned from,
supersedes) or the palette ("Add link: …"), picking the other end by search, and removed with
their ×. On the board the wheel scrolls a column and Shift+wheel scrolls across.
`Ctrl+K` opens the command palette: type a few letters to run any action that is legal from
where you stand (acknowledge, move, set priority, go to a view, group, order, new task), or to
find any task, lesson, decision or finding by key (`LIV-12`, `12`), by words as you type them,
and by meaning when the model is installed; `Enter` takes you there. `Ctrl+?` lists every
shortcut; the ones used most:

| Keys | |
|---|---|
| `Ctrl+K` | command palette and search |
| `j` `k` | move |
| `Enter` `Esc` | open, close the detail panel |
| `a` | decide: acknowledge, ratify, accept |
| `s` | ratify as a standing rule |
| `x` | reject |
| `n` | new task |
| `c` | add a note |
| `1`-`4` | priority |
| `/` | filter menu |
| `g i` `g a` `g t` `g l` `g d` `g f` `g r` | Inbox, Activity, Tasks, Lessons, Decisions, Findings, Graph |
| `+` `-` `0` | zoom the graph in, out, to fit (drag or scroll to pan, Ctrl+scroll to zoom) |
| `b` | list or board |
| `h` | show dismissed work |

## Develop

```
scripts/check              # format, lint, every test, benchmarks and model tests, each timed
scripts/check --install    # then install livtk and livtk-app
```

After an edit to the app, `scripts/check` takes about 12 s and `--install` about 64 s, 50 of
them building the installed app with link-time optimisation. The UI tests run in debug only
(GPUI records the debug selectors they find elements by only there). The CLI's benchmarks time
the installed binary, start-up included, so they run in release; the other benchmarks run in
the `quick` profile, release speed without LTO, which rebuilds in seconds. Installing reuses
`target/`. The model tests run when the model is in `~/.cache/livtk/models` (or
`LIVTK_TEST_MODELS`).

Builds go to `target/`. Cargo never prunes it and it grows with each toolchain and profile, so
run `cargo clean` when it gets large; the next build takes a few minutes.

## Licence

MIT
