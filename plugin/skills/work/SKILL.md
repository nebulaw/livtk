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
6. **Review.** `livtk move KEY review`, then use the `livtk:review` skill.
   If the task is part of an active project, livtk says so as it moves: write the project's
   update, how it stands, what moved and what is next:
   `livtk project update PRJ-1 --health on_track "parsing done; next: write the tasks"`
   (`at_risk` or `off_track` when it is, with why).
7. **Finish.** If the review upholds every point: `livtk done KEY`, and commit the work together
   with `.livtk/`. If it refutes one: `livtk move KEY doing`, note what failed, fix it, and review
   again.

One task at a time. Then `livtk next`.
