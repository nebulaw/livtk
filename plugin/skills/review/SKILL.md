---
name: review
description: Check a livtk task in review against its acceptance check with the independent refuter agent, and record the verdict. Use when a task is in the review state.
---

# Review a livtk task

The author never grades their own work. The refuter starts from a fresh context and tries to
prove the task is not done.

1. `livtk show KEY` for the acceptance check.
2. Spawn the `livtk:refuter` agent. Give it the task key and how to see the change (for example
   `git diff`, or the commit range). Give it nothing that claims the work is correct or tested.
3. Its verdict is recorded as a note on the task. Read it.
4. Every point UPHELD: `livtk done KEY`. Anything REFUTED: `livtk move KEY doing` and fix what it
   found. UNVERIFIABLE: run what would settle it, or ask the human.
