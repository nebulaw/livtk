# Agents in the app: assign, start, watch, steer

From TAS-65, 2026-10-21. The question: how should the app run Claude on tasks, so the developer no longer
keeps a terminal open to watch for acks? They want to create a task in the app, give it to an agent (or take
a suggested one), start it by hand or automatically, see the sessions that are running, and manage them.

## The short answer

Claude Code already ships a local supervisor for background sessions:

- `claude --bg "prompt"` starts one.
- `claude agents --json` lists them, each with its state: working, needs input, idle, done, failed or
  stopped.
- `claude attach <id>` opens one to watch and answer.
- `claude stop`, `claude respawn` and `claude rm` manage them.
- Each session moves into its own git worktree before it edits anything.

livtk should use that supervisor to run sessions rather than build its own. It adds three things:

- **Who the work is delegated to:** an agent on the task, which you pick or accept.
- **When it starts:** by hand, or automatically once you ack.
- **An Agents view:** the runs, with the ones that need you on top.

To watch or answer, the app opens `claude attach` in the terminal it already has. The log keeps only what
lasts: which run worked on which task, and how it ended. Live state is read from the supervisor and never
written.

## What the tools do

| Tool | Shape | Worth borrowing |
|---|---|---|
| GitHub Copilot agent | Assign an issue; one session, one branch, one PR; a "mission control" list; steer between tool calls; Stop keeps the commits | The task list with "needs your input" |
| Linear agents | The agent is the issue's **delegate**, not its assignee, so the human stays responsible. Sessions are pending, active, awaiting input, error or complete. Activities are thought, question, action, response or error. The agent must show a sign of life within 10 s | The delegate model, the states, instant feedback, stopping at once when asked |
| Codex, Cursor, Devin | One isolated workspace per task. Review the diff, then a PR. Devin plans before it spends compute | Plan first: in livtk that is already the ack |
| Conductor | Mac app: one workspace per task (a worktree, a terminal, a diff); see who is working and who is stuck | The local, worktree-per-task loop |
| Factory | Autonomy levels: the riskiest action that may run without asking | Permissions by stage, not by action |
| Claude Code | Agent view: rows for working, needs input, idle, completed, failed, stopped; peek and reply without attaching | Its states, as they are |

What they share:

- a list of runs, each with a state;
- a "needs you" flag;
- one run per task, in an isolated checkout;
- steering by a message, and a Stop button;
- a diff as the output;
- the person owns the work and the agent is a delegate.

## Where the human sits

- **The gates stay where livtk has them:**
  - you ack the plan;
  - the refuter gives its verdict;
  - you mark the task done.

  An agent never passes one of your gates. This is what the actors and the guard already enforce.
- **Permissions are set per stage, not asked per action:**
  - planning runs read-only (`--permission-mode plan`);
  - building may edit inside its checkout;
  - review is read-only.

  A prompt that still comes up turns the run into **Needs you**: its row goes to the top and a toast
  appears. There is no separate approvals inbox.
- **Evidence:** in Anthropic's study of agent autonomy (2026-02-18), experienced users approve less and
  interrupt more. They delegate, watch and step in. What helps is visibility you can trust and a quick way
  to intervene, not more approval steps.

  People can supervise several agents but review one significant change at a time (Simon Willison,
  2025-10). Hence a limit on how many build at once, **2 by default**. Reviews and research don't count
  toward it, because they don't edit.

## How it fits livtk

- **Agents** are the plugin's agent definitions: implementer, refuter, researcher. Planning is a
  session running the plan skill.

  Their writes are already signed `claude:<agent>`, and the guard already keeps them from your moves.
- **The suggested agent** comes from the task's state:

  | State | Suggested run |
  |---|---|
  | backlog | plan it |
  | acked | build it (implementer) |
  | review | review it (refuter) |
  | a research task | researcher |

  You can pick another.
- **Delegating** sets a prop on the task, `agent`. You stay the owner. This needs no new event type.
- **A run** is one session doing one stage of one task. Its id, agent and stage are written when it
  starts, and how it ended (done, failed, stopped) when it ends. While it runs, its state (working, needs
  you, idle) is read from `claude agents --json`.

## In the app

- **Agents**, a new place in the sidebar:
  - **Rows:** task key and title, agent, stage, state, how long it has run. Grouped Needs you, then
    Working, Idle, Finished.
  - **The panel:** the task, and the run's actions:
    - **Watch**: `claude attach` in a terminal over the middle, where you can answer. Esc comes back.
    - **Stop**: ends the turn cleanly and keeps the work.
    - **Resume**.
    - **Remove**.
- **A task's detail** gains:
  - an **Agent** field showing the suggestion, which you can change;
  - **Start**, which appears once the task may run: planning from backlog, building once acked, review in
    review;
  - **Stop** while a run is going.

  A "starting…" row appears at once, so a click always shows something.
- **Settings › this project:**
  - start automatically (off by default);
  - how many build at once (2);
  - where builds run: their own worktree, or the project's checkout.
- **Automatic start:** when you ack a task that has an agent, it starts. When it reaches review, the
  refuter starts. Starts beyond the limit wait in a queue, shown in Agents.
- **What goes away:** the terminal running `scripts/watch` and waiting for acks. The app starts the work
  itself.

## The hard parts

1. **Worktrees and the log.** `.livtk/events.jsonl` is in git. An agent working in a worktree would write
   to the worktree's copy, and the app would not see its notes until the branch merged. The fix is for
   `livtk` run inside a worktree to write to the main checkout's log. The guard already rewrites
   commands, so it can add `-C`.

   A built change then lives on the run's branch. When you mark the task done, livtk merges it,
   fast-forward only, or says why it can't. Running in the project's checkout avoids all of this, but
   only one build can run at a time.
2. **The stop check.** The refusal to stop with unrecorded work currently fires only for a subagent. A
   session started as the implementer itself needs the same check when it stops.
3. **A research preview.** The supervisor's command-line interface may still change, so the app talks to
   it through one small module. If `claude --bg` is missing, the app falls back to running `claude`
   in its own terminal. That session ends when the app quits, which the app says plainly.
4. **Unverified until tried:**
   - whether `--bg` accepts a fixed `--session-id`, a plugin agent (`--agent livtk:implementer`) and
     `--permission-mode`;
   - whether it prints an id the app can read;
   - whether an environment variable naming the task reaches the hooks.

   The installed CLI is 2.1.280, and the docs describe up to 2.1.302.

## The work, if this is accepted

1. **Spike: the supervisor as livtk would use it.** Try each item in (4) on this machine. Record what
   holds as a finding, and narrow the plan where it doesn't.
2. **Runs in the core and CLI:**
   - the `agent` prop and the suggestion rule;
   - run records;
   - `livtk agent set KEY implementer`, `livtk run KEY` (starts through the supervisor), `livtk runs`
     (the supervisor's live state joined to tasks), `livtk run stop KEY`;
   - the guard writing to the main log from worktrees;
   - the stop check for sessions started as the implementer.
3. **The Agents view and a task's Agent field** in the app: the rows, Watch in the terminal, Start, Stop,
   Resume, and "starting…" at once. Behaviour tests only.
4. **Automatic start and the queue:** the three project settings, the start on ack and on review, the
   limit, and merging a done build's branch.
5. **Retire `scripts/watch`** and update the skills and the README to the new loop.

## Open questions for you

1. Build in a worktree per run (several at once, merged at done), or in the project's checkout (one at a
   time, as today)? The plan supports both and **suggests worktrees**.
2. Automatic start off by default, so you turn it on per project. Agreed?
3. At most 2 builds at once by default?
4. Should planning also start from the app (backlog → plan run)? Your request suggests yes.
5. Is it all right to rely on Claude Code's research-preview supervisor, with a terminal fallback?

## Sources

All fetched 2026-10-21. The Claude Code pages carry no date.

- Claude Code: [agent view](https://code.claude.com/docs/en/agent-view), [agents](https://code.claude.com/docs/en/agents),
  [headless](https://code.claude.com/docs/en/headless), [CLI reference](https://code.claude.com/docs/en/cli-reference),
  [hooks](https://code.claude.com/docs/en/hooks), [worktrees](https://code.claude.com/docs/en/worktrees),
  [Agent SDK sessions](https://code.claude.com/docs/en/agent-sdk/sessions), [user input](https://code.claude.com/docs/en/agent-sdk/user-input)
- [Measuring AI agent autonomy in practice](https://www.anthropic.com/research/measuring-agent-autonomy), Anthropic, 2026-02-18
- [Parallel coding agents](https://simonwillison.net/2025/Oct/5/parallel-coding-agents/), Simon Willison, 2025-10-05
- Linear: [agents](https://linear.app/developers/agents), [agent interaction](https://linear.app/developers/agent-interaction), [guidelines](https://linear.app/developers/aig)
- GitHub: [mission control for Copilot agent tasks](https://github.blog/changelog/2025-10-28-a-mission-control-to-assign-steer-and-track-copilot-coding-agent-tasks/) (2025-10-28),
  [managing agents](https://docs.github.com/en/copilot/how-tos/copilot-on-github/use-copilot-agents/manage-and-track-agents)
- [Codex cloud](https://learn.chatgpt.com/docs/cloud), [Conductor](https://www.conductor.build/docs/), [Sculptor](https://imbue.com/product/sculptor),
  [Factory auto-run](https://docs.factory.ai/cli/user-guides/auto-run), [Devin 2026 notes](https://docs.devin.ai/release-notes/2026)
