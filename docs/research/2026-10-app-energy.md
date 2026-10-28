# What the app costs the laptop

TAS-119, 2026-10-27. The question: why does livtk use so much power, and what in its design
makes it so? Measured on an Apple-silicon Mac with `scripts/energy` (a separate copy of the app
on a test project, its own config folder, each state left alone for ten seconds after settling)
and `sample` (macOS's stack sampler).

## Before and after

| State, at idle         | Build 290 (before) | After TAS-119 | After TAS-120 (file events) |
|------------------------|--------------------|---------------|-----------------------------|
| Tasks, list            | 1.5% CPU           | 0.8–1.1% CPU  | 0.3% CPU, 0 wakeups/s       |
| Tasks, board           | 1.0% CPU           | 0.8% CPU      | 0.3% CPU, 0 wakeups/s       |
| **A task open**        | **27.5% CPU**      | 1.2–1.4% CPU  | 0.3% CPU, 0 wakeups/s       |
| The CLI (`list`, `show`, `sessions`) | 9–11 ms | 9–12 ms   | 9–11 ms                     |

After TAS-120, a change written from outside (the CLI) reaches the app in 0.27 s, through the
OS's file events alone.

**The wakeups in that column were measured with the windows not visible (FND-11).** Build 295,
measured 15 s after start with its window visible, takes 0.4–0.6% CPU and about 14 wakeups a
second. The app's own loop wakes 6 times in 15 s then, and draws 6 frames. The rest comes from
GPUI's display link (CVDisplayLink): it runs while the window is visible and pokes the main
thread on every tick, even when no frame is wanted. Covered, minimised or hidden, it stops. Bringing
a visible idle window to near 0 wakeups means stopping the display link when nothing is
invalidated, which is a change in GPUI.

Your own app, as you used it (a task open), measured 28–35% CPU idle before the fix.

## What it was, by share

1. **An animation that never ended: about 26 points of the 27.5%.** A task's panel shows its
   state graph. Its threads "breathed" in an animation that repeated forever, at 24 frames a
   second, whenever Reduce Motion was off. In GPUI an animation running anywhere in the window
   lays out and draws the *whole* window each frame: Taffy's flexbox layout, the glyph atlas,
   sprite sorting, everything. The fix: the threads breathe only while the pointer is over the
   graph, and are still otherwise.
2. **Work done on every draw that only changes on an event: most of what was left in a panel.**
   The task panel worked out every session's name (TAS-116) each time it was drawn. That ran a
   store query for up to 2,000 events per session, sorted in SQLite's temporary tables, so it
   even *wrote to disk* while idle. In one 8-second sample of an open panel, 940 of 1,137
   samples of drawing were this. The fix: session rows are worked out once, and kept until the
   log, the transcripts or Claude Code's live list change.
3. **Polling: the floor that remained, about 1% (fixed by TAS-120).** Every 200 ms the app checks the log for
   changes (`tick`), the docs, the menus, runs hooks and catch-ups, and saves its state if it
   changed. Every 2 seconds it surveys runs: it settles each project and asks `ps` about live
   turns. Each is cheap, but together they woke the app 5–30 times a second forever. TAS-120 replaced
   the poll with the OS's file events (FSEvents through the `notify` crate) on each project's
   `.livtk/`, its runs and its docs. The loop wakes on a change, every 200 ms only while
   something on screen moves on time, every 2 s while a run's turn goes, and otherwise once a
   minute as a safety net.

Also found along the way:
- **The endless animation was hiding bugs.** Three app tests passed only because it kept the
  window drawing: a panel's slide-in never finished in a test window without it. The tests now
  ask for reduced motion, as the rest do.
- **The CLI and the hooks are already fast** (about 10 ms a call), so they aren't where the
  power goes.

## What the app should be, and the decisions to take

The causes are three habits. Each has a rule that keeps it from coming back:

1. **An idle app draws nothing.** Every animation ends. An effect that's only felt (the graph's
   breath, a glow) plays on interaction or for a moment after a change, never forever. GPUI
   redraws the whole window for any animation, so a forever-animation is never cheap, however
   small it looks.
2. **Render only reads.** What a view shows is worked out when what it comes from changes, in
   the model, and kept; render never queries the store or walks the log. Today's code does
   still do work in render (timelines, flows, rows), unmeasured; the audit is a follow-up.
3. **Wait, don't poll.** The app learns of a change from the OS: file-system events on
   `.livtk/events.jsonl`, a run's stream and the docs folder; a process's exit for a run's turn.
   A timer is only for what has no event: an update check every 6 hours.

Proposed as decisions to accept: DEC "An idle app draws nothing: every animation ends",
DEC "Render only reads: derived data is worked out on change, in the model", and
DEC "The app waits on events, not timers".

## Follow-up tasks (PRJ-4), by what they save

1. **Wait on file-system events instead of polling.** Replace the 200 ms poll and the 2 s runs
   survey with FSEvents watches (the `notify` crate) and a process-exit wait for each turn.
   Expected: the remaining ~1% idle to near 0, and no 5–30 wakeups a second.
2. **Render only reads: audit and fix.** Count the work each draw does: the timeline, the
   state flow's layout, the list's rows, the Activity feed, `load_detail` per frame or per open.
   Move whatever is per-draw into the model's change handling, with a test that counts store
   queries during draws (zero).
3. **No frame without a change, kept by a test.** A frame counter (debug builds and tests): a
   test leaves each view idle and asserts no frames are drawn. Every `with_animation` and
   `.repeat()` is checked to end.
4. **Energy in the release path.** `scripts/check --energy` exists now (budget 2% idle per
   state). Run it in `scripts/release` (the full one), and tighten the budget to 0.5% once (1)
   lands.

## How to measure

- `scripts/energy`: idle CPU and wakeups per second of a separate app copy on a test project, in
  three states, and CLI call times. `--budget PCT` fails over it. `--settle SECS` (15 by
  default) waits past start-up first. Its window is visible, so the display link's wakeups
  count (FND-11).
- `scripts/check --energy`: builds a quick bundle and checks the budget (opens windows for
  about half a minute, so it's asked for, not run every time).
- `sample <pid> 5`: the stacks, to see where the time goes. `LIVTK_OPEN=KEY` opens a node's
  panel at start, to measure it.
