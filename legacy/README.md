# legacy/

The first livtk app, kept for reading only. Nothing here is built, linted or tested: it is out of
the workspace, and `scripts/check` never touches it. Its successor is Liv (`crates/liv`, proposal
0003); ideas worth keeping are rebuilt there, never copied.

- `livtk-app/`: the app and its tests, as they were at the last build released as livtk.app.
- `livtk-update-helper/`: the small program bundled inside livtk.app that installed its updates.
  Liv finishes its own updates (`liv --finish-update`), on the same `livtk-update` library.
- `scripts/`: what built, released and measured livtk.app (`bundle-app`, `release`, `energy`,
  `scale`). They expect the old layout and won't run as they are.

The livtk.app installed on this machine keeps working as it is, and the `livtk` command on PATH
is a link into it. Once Liv takes over (TAS-137), this folder goes.
