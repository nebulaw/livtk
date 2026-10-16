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

The human's control surface: an Inbox of everything waiting for you, tasks as a list or a board,
lessons, decisions and findings, and a detail panel with the plan, its links and its activity.
Changes from Claude show up while it is open, and flash once. It wears Linear's darkest palette.
As in Linear, the Filter menu (`/`) narrows by title, key or priority, and the Display menu
holds Grouping (state, priority) and Ordering (priority, last update, creation), each its own
menu, and whether dismissed work shows; groups are solid bands that collapse on a click.
Double-click a title or body in the detail panel to edit it: Enter (Ctrl+Enter for the body)
or clicking away saves, Esc cancels. Editing an acknowledged plan sends it back to planned. Drag a row
onto another group, or a card onto another board column, to move it; only legal moves light up.
Done work stays in view; what was dismissed (canceled, rejected, retired) shows with `h`.
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
| `g i` `g t` `g l` `g d` `g f` | Inbox, Tasks, Lessons, Decisions, Findings |
| `b` | list or board |
| `h` | show dismissed work |

## Develop

```
cargo test                                           # core and CLI
cargo test -p livtk-app                              # the app, with headless UI tests
cargo test --release -p livtk-cli -- --ignored       # the speed budget on 10k tasks
cargo clippy --all-targets && cargo fmt --check
```

Builds go to `target/`, about 3 GB for everything (dependencies carry no debug info). Cargo
never prunes it and it grows with each toolchain and feature set, so run `cargo clean` when it
gets large; the next build takes a few minutes.

## Licence

MIT
