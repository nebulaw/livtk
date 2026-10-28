# Architecture

livtk is where a human and AI agents build a product together: the work, its gates, its history
and what was learned live in the repository, and the human decides only what is truly theirs.
This page is the map of the code: where things are, and the rules between them. The why behind
it is in `docs/proposals/0002-simplify.md` (the direction) and `0003-ui-framework.md` (the UI).

The code is moving to the shape 0002 §5 describes. This page says what is true now, and marks
what is still to come.

## The crates

```
crates/
  livtk-core     the domain, the log and the projection, and the Workspace API over them
  livtk-cli      the `livtk` command and the Claude Code hooks
  livtk-update   updates: the manifest format, staging, the swap, where each app keeps its data
  livtk-build    which build this is (version, build number, commit)
  livtk-ui       the design system: tokens, themes, components. Knows nothing of tasks
  liv            Liv, the new app, on livtk-ui (being built: PRJ-6)
  livtk-app      the app in use today, until Liv takes over (then deleted)
xtask            the project's own tasks: `cargo xtask icons | fixture | liv`
plugin/          the Claude Code plugin: skills, agents, hooks
```

Still to come (0002 §5.1): `livtk-core` splits into the domain (no I/O), `livtk-store` (the log
and the projection) and `livtk-service` (the commands and queries every surface calls).

## Rules between them

These are absences: things that are never true. A change that breaks one changes this page
first.

- **`livtk-ui` knows nothing of tasks.** It never depends on `livtk-core`. The app maps its
  concepts (a task's state) onto the UI's (a `Tone`).
- **Liv never imports gpui-component.** It reaches it only through `livtk-ui`, so that crate's
  API changes stop there.
- **Liv and livtk.app share nothing they write.** Each has its own identifier, settings, state,
  log and update channel (`crates/liv/src/identity.rs`). Both read the same projects.
- **A view never queries the store** while drawing (DEC-7). It reads what its store worked out
  on change.
- **Nothing polls** (DEC-8). The apps wake on file events, and draw nothing while idle (DEC-6).

## Building and running

- `scripts/check`: format, lint, every test, the benchmarks.
- **The app in use:** `scripts/release --quick` builds `livtk.app` and puts it in livtk's
  update channel.
- **Liv:** `cargo xtask liv` builds `Liv.app`. `--install` puts it in ~/Applications;
  `--release` publishes it to Liv's own channel.
