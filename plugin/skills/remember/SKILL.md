---
name: remember
description: Propose a lesson, a decision or a finding in livtk so it outlives this session. Use when the human corrects you, states a standing rule, makes a choice worth keeping, or when research turns up something reusable, in a project that has a .livtk directory.
---

# Remember through livtk

Memory binds only what the human approves. You propose; they ratify or accept.

| What | When | Command |
|---|---|---|
| **Lesson** | A correction or rule that should hold beyond this task | `livtk lesson "<the rule, one sentence>" --why "<what happened>"` |
| **Decision** | A choice between options, with its reasons | `livtk decision "<the choice>" --body -` (context, options, why) |
| **Finding** | Something found out that will be useful again | `livtk finding "<what>" --body "<details and sources>"` |

- Write a lesson's title as the rule itself, in the imperative: "Run cargo check before reporting
  a task done", not "cargo check".
- Link where it came from: `--from KEY` (the task, or the finding behind it).
- If it replaces an earlier lesson or decision, say so: `--supersedes KEY`. When the human
  ratifies or accepts the new one, the old one retires.
- Before proposing, check it is not already there: `livtk recall "<the rule>"`. If it is, add a
  note to the existing one instead (`livtk note KEY "seen again: ..."`).
- Then tell the human in one line what you proposed and that `livtk inbox` lists it. You never
  ratify, reject, retire or accept.

Ratified lessons and accepted decisions come back on their own: the relevant ones are added to
each prompt, and the human can mark a lesson as a standing rule shown at every session start.
