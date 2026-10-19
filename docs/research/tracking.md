# Tracking above the task

Research for TAS-26, 2026-10-19. The question: what should livtk have above the task, so progress
is observable, without the concept sprawl of the big tools, for one developer up to a large
team, when Claude does much of the work?

## The short answer

Keep the task as the unit of work, and add one concept above it: the **project**. A project is
an outcome with a done-condition, an optional target date and an owner, and projects can nest. Make
progress observable through two things: progress computed from the project's tasks, and short health
updates that Claude writes and the human reads. Add no cycles, no estimates, no story points and
no separate initiative or milestone kinds. A nested project covers what those cover. The
scarce resource to show is human attention: what waits on the human, at which gate, and for how long.

## What the tools do

**Linear** layers three levels: issue → project → initiative. Cycles are a separate,
optional time axis. ([projects](https://linear.app/docs/projects),
[initiatives](https://linear.app/docs/initiatives), [cycles](https://linear.app/docs/use-cycles))
- **Projects** have one lead, start and target dates (as loose as a year), and optional
  [milestones](https://linear.app/docs/project-milestones) with their own completion %.
- **Initiatives** replaced roadmaps in 2024
  ([changelog](https://linear.app/changelog/2024-06-25-introducing-initiatives)).
- **Opt-in:** cycles, triage and estimates are all off by default or optional per team.
- **Paywalled:** much of the tooling for large organisations needs the Business or Enterprise plan.
- **Method:** the Linear Method says "create momentum, don't sprint", scope projects to "1–3 weeks
  with 1–3 people", and "if it's not a task, it doesn't belong in the issue tracker"
  ([method](https://linear.app/method/introduction),
  [scope](https://linear.app/method/scope-projects),
  [issues](https://linear.app/method/write-issues-not-user-stories)).

**Shape Up** (Basecamp) runs six-week cycles with a two-week cool-down. Its core ideas:
- **Appetite instead of estimates:** fixed time, variable scope.
- **No backlog:** pitches that aren't picked are dropped.
- **A circuit breaker:** a project that misses its cycle isn't extended.
- **Hill charts** show "figuring out" versus "making it happen" rather than percent done
  ([Shape Up](https://basecamp.com/shapeup), [hill charts](https://basecamp.com/shapeup/3.4-chapter-13)).

Reports from outside Basecamp go both ways:
- **For:** UserVoice reports twice the features shipped
  ([UserVoice](https://uservoice.com/blog/adopting-shape-up)).
- **Against:** Trustpair dropped it because every project ending together made QA a bottleneck
  ([Trustpair](https://jobs.trustpair.com/posts/why-the-shapeup-method-just-wasn-t-for-us)).
- **Limits:** shaping depends on a few senior people, and it scales poorly past about 50 people
  ([curiouslab](https://www.curiouslab.io/blog/basecamp-shape-up-myths/)).

**Scrum and Jira** use sprints, story points, velocity and epics. Jira Plans adds levels such as
Initiative → Epic → Story for large organisations
([Atlassian](https://www.atlassian.com/software/jira/guides/advanced-roadmaps/overview)). It helps
when many dependent teams share fixed external dates. Elsewhere it is mostly ceremony. Ron
Jeffries, who may have invented story points, apologised for them in 2019
([summary](https://www.allstacks.com/blog/story-points-got-an-apology-from-the-man-who-invented-them)).

**GitHub** has sub-issues (8 levels deep), organisation-wide issue types, iterations and a
roadmap view ([changelog](https://github.blog/changelog/2025-04-09-evolving-github-issues-and-projects/)).
Open source uses milestones as release payloads, not time boxes. Rust keeps one milestone per
version, and its goals live in a separate repo ([rust milestones](https://github.com/rust-lang/rust/milestones)).

**Others:**
- **Asana:** goals with progress fed from projects, and portfolios that roll up status.
- **Plane:** separates time (cycles), theme (modules) and goal (initiatives) into three axes
  ([Plane](https://docs.plane.so/core-concepts/projects/initiatives)).
- **Height:** an AI-first tracker, shut down in 2025
  ([announcement](https://x.com/height_app/status/1903820182557999555)). The cause is unknown,
  but it's a caution.

## What makes progress observable

Two mechanisms recur and both work:

1. **Computed progress.** Linear counts issues done against remaining. Work in progress counts
   as ¼. It predicts completion from recent velocity with roughly ±40% bands
   ([project graph](https://linear.app/docs/project-graph)). Milestones carry their own %.
   - **Shape Up's alternative:** uphill versus downhill (unknowns left versus known work left).
     This says more than a percentage when the known work is quick, as it is with agents.
2. **Written health.** Linear's project updates have three parts
   ([updates](https://linear.app/docs/initiative-and-project-updates)):
   - A health flag: On track, At risk or Off track.
   - A short text.
   - A nudge on a schedule, with "Update missing" shown when one is late.

   Large teams rely on this. Leads post weekly, and syncs often just read the updates
   ([morgen](https://www.morgen.so/blog-posts/linear-project-management)).

What teams drop:
- **Estimates:** story points correlated strongly with time in only 7% of 16 open-source
  projects ([Tawosi et al. 2022](https://solar.cs.ucl.ac.uk/pdf/tawosi2022esem.pdf)).
- **Cycle planning:** "if you moved to Linear to escape Scrum and find yourself doing cycle
  planning every other Monday, the ceremony followed you"
  ([FlowBoard, a competitor](https://dev.to/karim_g/the-best-linear-alternatives-for-teams-that-outgrew-cycles-c77)).
- **Deep hierarchy:** one HN user found "projects/tasks/subtasks … less useful than just
  dependencies" ([HN](https://news.ycombinator.com/item?id=38294623)).
- **Neglected target dates:** reported only by a competitor, ONES
  ([ONES](https://ones.com/blog/linear-app-project-management-features-examples-common-mistakes/)).

The evidence that holds up best is DORA's: small batches, stable priorities and few concurrent
goals. Unstable priorities cost productivity and cause burnout, and technical excellence doesn't
make up for it ([DORA 2024](https://dora.dev/research/2024/dora-report/)).

## What changes when agents do the work

Every vendor gives agents an **issue or ticket that ends in a pull request a human reviews**,
never a sprint or an epic:
- **GitHub Copilot:** the issue creator can't be the final approver
  ([best practices](https://docs.github.com/copilot/how-tos/agents/copilot-coding-agent/best-practices-for-using-copilot-to-work-on-tasks)).
- **Linear:** when an issue is delegated to an agent, the human stays the assignee and the agent
  is a contributor ([agents](https://linear.app/agents)).
- **Devin:** does best on discrete, well-scoped tasks and badly on ambiguous projects or
  mid-task changes ([Cognition](https://cognition.com/blog/devin-annual-performance-review-2025)).
- **Claude Code:** the recommended loop is to plan first, have a human review the plan, then
  execute ([Anthropic](https://claude.com/blog/how-anthropic-teams-use-claude-code)). That is
  livtk's plan → ack.

The bottleneck moves from writing code to **specification and review**:
- **DORA 2025:** 21% more tasks completed, but review time up 91% and delivery flat
  ([summary](https://www.faros.ai/blog/key-takeaways-from-the-dora-report-2025)).
- **GitHub:** "judgment is the bottleneck"
  ([GitHub](https://github.blog/ai-and-ml/generative-ai/agent-pull-requests-are-everywhere-heres-how-to-review-them/)).
- **METR:** developers felt 20% faster and were 19% slower
  ([METR](https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/)). So measure
  the queue, not the feeling.
- **Linear's CEO** (secondary report): "the bottleneck shifts from execution to deciding what to
  build" ([digest](https://www.tbpndigest.com/story/2026-03-26/linear-ceo-karri-saarinen-declares-issue-tracking-is-dead-and-reveals-linears-ai-era-pivot)).

What loses value and what gains it:
- **Loses:** story points, velocity and sprint commitments. A task that finishes in an hour
  makes a time box meaningless.
- **Gains:**
  - Clear outcomes and acceptance criteria.
  - A plan approved before work starts.
  - Review gates, tests and small batches.
  - A record of decisions.

  livtk already has all of these at the task level: the plan note, the ack, the refuter, the
  human's done, decisions and lessons. What it lacks is the layer that says *why these tasks*
  and *how far along the outcome is*.

## Recommendation for livtk

**One concept: the project.** It is a node kind, so it gets the log, keys, links, search, the
graph and the panel for free.

- **What it holds:**
  - A title and an outcome (its body), ending in a **"Done when"** line: the acceptance for the
    whole outcome.
  - An optional target date and an optional owner (a human). Claude never owns the outcome.
- **States:** proposed → active → done, plus paused and dropped. Moving from proposed to active
  is a human gate, like a task's ack. Claude may propose a project, but only the human commits
  the team to it. Moving to done is the human's call, like a task's.
- **Tasks join** with the existing `parent` link (task part of project). A task can belong to
  one project. Tasks that belong to none are fine: small work needs no project.
- **Projects nest** through the same link. A project inside a project covers Linear's milestone
  and initiative without new kinds:
  - A large effort is a project of projects.
  - A checkpoint is a small project whose done-condition is "the human has looked and decided
    what next".
- **Progress is computed, never typed in:**
  - The tasks done out of all its tasks, with nested projects counted through.
  - **Waiting on you:** how many of its tasks sit at each human gate (to ack, to check in review)
    and for how long. Agents make known work quick, so the queue at the human is the real progress
    signal, and no tool shows it well today.
- **Health is written:**
  - **Who writes:** Claude adds an update when a task under the project moves to review or done,
    and when the human asks.
  - **What it holds:** a health flag (on track, at risk, off track), what moved, and what is next.
  - **Where it lives:** updates are notes in the project's history.
  - **Staleness:** an active project with no update in a week shows "update missing". No
    reminders are pushed.
- **Few at a time:** the projects view puts the active ones on top. With more than three active,
  it says so plainly. Fewer concurrent goals and stable priorities are what the evidence
  supports, and it's a nudge, not a lock.

**In the app**, following the app's grammar:
- **The Projects view** is a list. Each row shows:
  - The project's state.
  - A progress bar.
  - Health.
  - The target date.
  - The count waiting on you.
- **The panel** shows:
  - The outcome and its done-condition.
  - The nested projects.
  - Its tasks, grouped by state.
  - The update history.
- **The Tasks view** gains "group by project".
- **The graph** already draws the parent links.

**For Claude:**
- When planning a task under a project, Claude reads the project's outcome, as it already reads
  linked docs.
- The plan skill learns to propose a project and split it into tasks, all waiting on the human's
  gates.
- The session brief lists active projects and what waits on the human.

**Scale without new concepts:**
- **One developer:** projects or none.
- **A small team:** a few active projects, each with an owner.
- **A large team:** projects of projects, plus a filter by owner. When teams come (milestone 6),
  a team is a filter over owners, not a container.

## What is left out, and why

- **Cycles and sprints.** Agent tasks finish in hours, so a two-week box measures nothing.
  Teams report the planning ceremony as overhead. A weekly cadence can still be read off the
  activity grid, and if a team asks, a cadence can come later as a view, not as a concept.
- **Estimates and story points.** They are weak predictors even for human work, and at one-hour
  agent tasks they are noise. Progress comes from counts and the human queue instead.
- **Initiative, epic and milestone as separate kinds.** Nesting projects covers them, and
  separate kinds are the hierarchy depth users complain about.
- **Triage as a separate inbox.** livtk's inbox already gathers what waits on the human. Intake
  from outside the team can wait until there is a team (milestone 6).
- **Appetite as a time budget.** Its useful half, a ceiling with a circuit breaker, maps better
  onto human attention than onto calendar time. It is worth revisiting once the "waiting on you"
  numbers exist. Until then a target date is enough.

## Follow-up tasks, if this is accepted

1. **Projects in the core and CLI.** The kind, its states and human gates, joining by `parent`,
   computed progress and waiting-on-you, and health updates. `livtk project new | list | show`,
   with progress in `livtk list`.
2. **Projects in the app.** The Projects view and panel, progress and health in rows,
   "group by project" in Tasks, and the active-projects nudge.
3. **Claude and projects.** The plan skill proposes and splits projects, a task's plan reads its
   project's outcome, updates are written when tasks move, and the session brief shows active
   projects and the human's queue.
4. **Flow numbers.** Time each task spends at each state and gate, the age of the review queue,
   and lead time per project, shown in the project panel and the activity view. This is
   measured rather than estimated, and it is where "is this going well" gets answered.
