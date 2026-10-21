---
name: plan
description: Turn a goal into livtk tasks that the human acknowledges before any work starts. Use when asked to plan, scope or break down work in a project that has a .livtk directory, and before starting any change there that is more than a few lines.
---

# Plan work as livtk tasks

The human acknowledges every plan before work on it starts. Your job here is a plan worth
acknowledging, then to stop.

1. **Understand the goal.** Run `livtk recall "<the goal>"` for rules and decisions that apply,
   and follow them. Read the code it touches. Ask the human only what the code cannot answer.
2. **Split it into tasks.** Each task is one reviewable change (about one commit) that ends in
   something checkable. Prefer a few clear tasks over many tiny ones.
3. **Create each task with a plan in its body:**

   ```bash
   livtk new "Title in plain words" -p 3 --body - <<'EOF'
   ## Goal
   What will be true when this is done, in one or two sentences.

   ## Approach
   The steps, and the decisions already made.

   ## Files
   What will change.

   ## Acceptance check
   - An observable check: a command and its expected result, or what is seen on screen.
   EOF
   ```

   Priority: 1 urgent, 2 high, 3 medium, 4 low.
4. **Add dependencies** where one task needs another first: `livtk dep add KEY --on OTHER`
   (or `--after OTHER` when creating). When a goal is too big for one task, keep it as a parent
   task and create its parts as sub-tasks with `--parent KEY`; each part is planned and
   acknowledged on its own. `livtk link KEY relates OTHER` notes a connection that is not an
   order. A task a doc describes or that changes a doc: `livtk doc link KEY docs/PATH.md`
   (`livtk doc list` shows the docs).
   **An outcome bigger than a few tasks is a project.** Propose it with its outcome and what
   "done" means, then create its tasks under it:

   ```bash
   livtk project new "The outcome in plain words" --body - <<'EOF'
   Why it matters, and what changes for the people it serves.

   Done when: the one observable condition that says it is reached.
   EOF
   livtk new "First task" --parent PRJ-1 --body - < plan.md
   ```

   A project waits for the human to commit to it (`livtk move PRJ-1 active`, theirs alone),
   as each of its tasks waits for its ack. A task belongs to one project; a project may sit
   inside another. Check `livtk project list` first: more than three active at once is worth
   saying to the human before adding a fourth.
5. **Move each task to planned:** `livtk move KEY planned`.
6. **Stop and ask.** Show the human the plan (`livtk list`, plus anything they need to judge it)
   and ask them to acknowledge with `livtk ack KEY` or in the app. You cannot acknowledge, and you
   do not start a task until it is acknowledged.

If a plan changes after acknowledgement, update it with `livtk edit KEY --body -`. It returns to
planned and needs a new acknowledgement; that is the point.
