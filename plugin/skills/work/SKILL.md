---
name: work
description: Carry acknowledged livtk tasks through to done, one at a time. Use when asked to work on, continue or finish tasks in a project that has a .livtk directory.
---

# Work through livtk tasks

1. **Pick the task.** `livtk next`, or the one the human names. Only an acknowledged task can
   start: `livtk start KEY`. If nothing is acknowledged, say so and ask; do not start anyway.
2. **Read the plan.** `livtk show KEY` gives the goal, approach and acceptance check. They are your
   brief. Read the docs it is linked to and the outcome of the project it is part of (`livtk
   start` names both): the work is written against them, and a change that alters what a doc says
   updates the doc too. If the plan turns out wrong, stop and update it (`livtk edit KEY --body -`, which sends it back for
   acknowledgement) instead of quietly building something else.
3. **Do the work.** A self-contained implementation step can go to the `livtk:implementer` agent.
4. **Verify.** Run the acceptance check yourself and look at the real result.
5. **Keep the record.** Write what you tried, what failed and what you decided as notes:
   `livtk note KEY "..."`. Notes never change the plan. Something that should hold beyond this
   task becomes a lesson, decision or finding (the `livtk:remember` skill).
6. **Review.** Commit the work first (with `.livtk/`): nothing goes to review uncommitted. Then
   `livtk move KEY review`, and use the `livtk:review` skill.
   If the task is part of an active project, livtk says so as it moves: write the project's
   update, how it stands, what moved and what is next:
   `livtk project update PRJ-1 --health on_track "parsing done; next: write the tasks"`
   (`at_risk` or `off_track` when it is, with why).
7. **Finish.** If the review upholds every point: `livtk done KEY`, and commit `.livtk/` with the
   record of it. If it refutes one: `livtk move KEY doing`, note what failed, fix it, commit, and
   review again.

One task at a time. Then `livtk next`.

## When livtk started you (a run)

The human may start you from the livtk app, or the project may start you on its own. You can
tell: `LIVTK_TASK` names your task. No one watches your terminal, so:

- **What you say last in a turn becomes a note on the task.** Make it what the human needs to
  read: what you did, what is left, what you need.
- **Need the human?** End your turn with your question. Their note is what you read next.
- **Need a command you may not be allowed to run?** Run it. If it is refused, livtk shows the
  human exactly what was refused, with Allow and Deny, and you go on once they answer. Never ask
  for permission in words: a note cannot grant it.
- **In a worktree** (your folder is under `.livtk/runs/worktrees/`): you are on your own branch.
  Commit everything there; livtk merges the branch when the human marks the task done, and leaves
  anything uncommitted behind.
- **Before review:** run the project's checks and commit, then `livtk move KEY review`. A build
  is not finished until it is committed.
