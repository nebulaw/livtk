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
  log and update channel. Liv's folders follow each platform's convention through `etcetera`
  (on macOS `~/Library/Application Support/com.github.nebulaw.liv`, its logs in
  `~/Library/Logs/com.github.nebulaw.liv`), and are set in `crates/liv/src/identity.rs`. Both
  apps read the same projects.
- **Liv's identity is written once.** `identity.rs` holds it; a test holds the bundle metadata
  (`[package.metadata.bundle]` in `crates/liv/Cargo.toml`) equal to it, and `cargo xtask liv`
  reads it from the built binary (`liv --identity`).
- **A view never queries the store** while drawing (DEC-7). It reads what its store worked out
  on change.
- **Nothing polls** (DEC-8). The apps wake on file events, and draw nothing while idle (DEC-6).

## Building and running

- `scripts/check`: format, lint, every test, the benchmarks.
- **The app in use:** `scripts/release --quick` builds `livtk.app` and puts it in livtk's
  update channel.
- **Liv:** `cargo xtask liv` bundles it with `cargo bundle` (install it once:
  `cargo install --locked cargo-bundle`), from the metadata in `crates/liv/Cargo.toml`, and signs
  it ad hoc. `--install` puts it in ~/Applications; `--publish [notes]` puts it, signed with
  this machine's own key (`.liv-dev/`, never committed), in Liv's local update channel, where
  the installed Liv finds it. Builds use the quick profile unless `--profile release`.
- **Liv's updates** (`crates/liv/src/updates.rs`, over `livtk-update`): every channel is signed
  (minisign), and Liv trusts only the keys built into it. Stable and preview come from GitHub
  Releases; local from this machine. A staged build finishes its own update once Liv quits.
