# The app's grammar

How livtk's app is put together, so every view reads as made by one hand. A new view is built
from these parts; one that needs something else changes this page first.

This grammar is for now. It will change to fit agentic work: views of who is working on what,
with their chats, terminals and the rest. When that comes, it changes here first.

## Three places

- **Left: where to go.** The sidebar: projects, the views, the tools. Navigation only, in
  sidebar rows (32px, rounded, 15px text). Nothing else uses the sidebar's look.
  Its foot holds the updates row, Shortcuts and Settings, each a **foot row** (below). The
  updates row is always there, in one of three states: **Check for updates** (a click looks, as
  the menu's Check for Updates… does), **Checking for updates…** (clicks do nothing until it is
  done) and **Restart to update · build N** in the accent once a build is staged. The collapsed
  rail has the same three states on one button; its dot shows only when a build is staged.
- **Middle: the thing itself, edge to edge.** One surface fills each content box: a list, the
  board, the graph, the terminal, a doc being read. No second sidebar, no columns nested inside
  it, no floating fields of its own. The middle can be split into panes (below).
- **Right: what is open.** The panel shows the one thing chosen in the focused pane: a task, a
  doc. Its properties, its links, its history. Metadata lives here, not in the middle.
  A node's detail can open three ways, as Notion opens a page (the button beside its close):
  side peek (this panel), center peek (a large card over the dimmed window; Esc or a press
  outside closes it, the list behind keeps the keys) or full page (in the list's place in the
  pane it was opened from; Esc gives the list back). The choice sticks.
- **A node named anywhere opens on a click.** A link, a sub-task's parent chip, a session's work
  or a doc's tasks all name a node the same way (state mark, key, title). A click goes to it in
  its own view, past a filter, a collapsed group or hidden dismissed work. A doc's tasks open
  beside the doc instead. A link's × only unlinks.

The flow is always the same: a list in the middle → open one → its details in the panel. A
thing that wants the whole middle (the terminal, a doc being read, a doc being edited) takes the
list's place, behind a header bar, and Esc gives the list back.

## Panes

- The middle splits into panes, side by side or stacked, each a view in its own content box,
  a gutter between them that drags. Each view shows in one pane at most.
- One pane is focused: the keys, the sidebar and the dock are its, and its box's border is the
  stronger one. A press in another pane focuses it.
- A split starts with a placeholder: the views to put in it, and a way to close it.
- While there is a split, its views sit grouped at the sidebar's top (sliding in as they join),
  in the split's order, the focused one lit; each has an unsplit button that closes its pane
  and puts the view back in its usual place. The collapsed sidebar's rail groups them the same
  way, the unsplit × at a button's corner.
- The sidebar picks a view the split has: its pane is focused. A view the split does not have
  shows alone over it, and the split comes back when one of its views is picked (or with the
  next pane's key, or closing the view alone). Splitting a view shown alone takes it into the
  split.
- The terminal is one view holding several terminals, as VS Code does: a list beside it once
  there are two, a click shows one, its × kills it.

## Agents and Sessions

The first of the agentic views: two views under one Agents heading in the sidebar, each with the
same three places.

**Agents** (G E) lists the agents Claude sessions can run as, by where they live: livtk's, this
project's, yours. The panel shows the one picked as its page: name, description, tools, model and
instructions, with Save, Copy and Remove. livtk's own agents can be copied, never changed or
removed. Remove asks once, on the button itself, before it removes.

**Sessions** (G S) lists every session of the project, past and running, with **Needs you**
first, then Working and Idle. The list is merged from three sources: Claude Code's transcripts
(title, when), its live list (where it stands, refreshed while the view is open) and the log (who
wrote what to which task). The panel shows the one picked: its standing and what it did in livtk,
with Read, Open (in the terminal) and Stop.

**Reading a session.**

- **Read** (or a double click) takes the list's place with the conversation: what was asked, what
  Claude said, each tool call one line until opened, its result under it. A call that started a
  subagent offers its work, which takes the session's place until Esc brings it back.
- It reads a page at a time, so a long transcript never stalls the window. Esc gives back the list.
- **From a task.** Its properties name the sessions that worked on it, and a click reads one from
  that task's first event in it, in Sessions.
- **Read-only.** livtk never writes Claude Code's files.

**A task's run, from its panel.** A task's properties have an **Agent** (any agent there is;
**Default**, livtk's own for the stage). Its run sits in the panel's title bar, right after its
key: Start (to plan, build or review, as its state is ready for), "Starting…" at once, then
where the run stands, with Stop while it goes.
Its notes are the conversation. The note a run waits on is marked **Needs you**, with Allow /
Deny when it asks to run a command; a question is answered with a note in the box below, which
the run reads next. A task Claude is writing on says **Writing…** in its row, and one that
waits says **Needs you** there and comes first in the Inbox.

**Quitting with agents working.** Quitting (Cmd-Q, or Quit livtk) while a run is writing asks
first, naming each: **Keep them working** (they go on without the app, which finds them again
when it starts), **Stop them all** (each turn ends cleanly, the work kept, a note on each task
saying the quit stopped it) or **Cancel**. On the next start, the runs a quit stopped are offered
back once: **Continue them** or **Leave them stopped**. Nothing is remembered silently.

## Settings

Settings are a place, not a modal (⌘, or the Settings row under Shortcuts): the sidebar becomes
their list of sections, with the way back at its top, and the middle shows the section, behind
a header bar; Esc goes back. Sections are yours (this app's, private) or this project's
(`.livtk/config.toml`, committed and shared), and each page says which. A project setting is
written through the core, checked, comments kept; one that does not hold says why and changes
nothing.

What a detail shows that some want and others want the room for (its state graph) is a personal
switch under Opening work, never a per-node toggle; hidden, nothing it offered is lost (the state
menu still moves it).

## Parts

Parts built to be reused live in `ui/parts`, one look each from theme tokens only; views compose
them and never restyle them.

- **Foot rows** (`ui/parts/foot_row.rs`): the sidebar's quieter rows under the views, a sidebar
  row's size. An icon (or the accent's dot), a label cut when narrow, then a key hint or a few
  `TEXT_4` words kept whole. Muted (`TEXT_4`, `TEXT_3` on hover) or accent (`ACCENT_HOVER`), and
  disabled (no hover) while there is nothing to do.
- **Chips** (`ui/parts/chip.rs`): a property as a 24px pill, a mark or icon then its value. Unset,
  it says the property's name in `TEXT_4` with a dashed edge; open, it is lit (`SELECTED`) while
  its menu shows. The new-node form's properties and Add link are chips.
- **Choice menus** (`ui/parts/choice_menu.rs`): a menu that picks one. A field on top finds a
  choice by typing (or types a new value, as a new owner); each row a lead, a label, quiet words
  after, the chosen one ticked; up and down move the highlight, Enter picks, Esc closes. What the
  choices are is the model's; the menu only draws them. The panel's Add link is one too.
- **Date picker** (`ui/parts/date_picker.rs`): a field for a date in words ("Dec 15", "next
  friday", "in 2 weeks", "eom", or ISO), the quick picks, a month's grid (today in the accent's
  text, the date set filled in the accent), the way to the month before and after, and Clear.
  The keys: up and down a week, left and right a day and Home and End the week's ends while its
  field is empty, Page Up and Page Down a month, Enter picks the date typed or the day the keys
  are on. Text it cannot read is said under the field, never guessed. Dates only, no times; the
  dates themselves are `model::dates`, tested on their own.
- **List rows**: 36px, full width, `px_4`, hairline between rows; selected is `SELECTED`, and the
  highlight glides from row to row. A row leads with the same slot (priority or kind), then key,
  state, title, and ends with when it changed. Rows say something at a glance, never a bare name.
  What a row knows beyond that sits after the title, small and quiet: a parent's sub-tasks done,
  a project's progress bar with done of all, its health, its target and what waits on you.
- **Groups**: solid `SURFACE` bands, the same height as a row, with a caret, a name and a count;
  click to collapse.
- **Header bars**: 46px, `px_4`, a hairline below: what this is on the left, its actions and close
  on the right. Every opened thing (the panel, a full-middle surface, an editor) has one.
- **Section labels**: `text_xs`, medium weight, `TEXT_4` ("Links", "Docs", "Mentioned in").
- **Buttons**: text buttons 24px (28px in empty states), `rounded_lg`, `text_xs`, an icon at 14px.
  Icon buttons brighten on hover (`icon_hover`) and carry a tooltip.
- **Chips and tags**: 20px, `rounded_full`, `BORDER_STRONG`.
- **Radii**: `rounded_lg` for rows, fields and buttons; `rounded_xl` for menus, popovers, bars
  that float, cards.
- **Empty states**: an icon, a line, what to do, and a button that does it.
- **New-node form**: one modal for every kind it makes, one grammar (prototype:
  `docs/prototypes/new-node-form.html`). Its head says where it goes (the project) and what it
  makes, with its close. Then its title and body, flush with the card's edge; the body grows
  with its text to a cap. Then one row of chips, the kind's properties then **Add link**: a task's
  State (Backlog or Planned), Priority, Project and Agent; a project's Status (Active or
  Proposed), Owner, Target date and Part of. Add link offers the relations the panel's Add link
  does, then the palette picks the node; the links show below the chips, grouped by relation,
  named as everywhere, × to take one back. A node has one parent, so a "Part of…" link and the
  Project chip take each other's place. Its foot: **Create more** (the form stays, emptied, its
  chips kept) on the left; Cancel and Create (⌘↵, dimmed until there is a title) on the right.
  N makes a task, P a project; the dock's main button and an empty list make what the view
  lists. A new kind joins the form as a row of `Making` with its `Prop`s, never as another form.
- **Timeline** (a node's Activity): one rail, oldest first, each event a 20px mark on it, each
  kind its own glyph in the same circle (`ui/timeline/kinds.rs` is the one table). What happened
  is solid. Two levels (`model/timeline.rs` holds the rules): the rail is the node's life (its
  making, every move outside an agent's own work, the human's words, bookkeeping); a lane is one
  stint, one agent's work, stepping off and back as a git graph's. A run's stint lasts from
  Started to Ended. An interactive agent's (its actor and conversation, so a refuter is its own)
  starts at its first event after a move and ends at its own move, its last mark. Every move
  resets to the rail, all but a run still going; lanes go side by side only for stints at once,
  never nested. Four
  or more small changes in a row fold into "N more changes"; notes are cards beside their
  avatar. The composer is the rail's next point, after the latest event; the states still ahead
  follow it, dashed and hollow. A dot travels the rail to where the node is now.
- **Composer** (the note bar, the rail's next point in a node's panel): one `rounded_xl` card on `SURFACE` holding
  the text and a send button, nothing else; a `BORDER` hairline, `BORDER_STRONG` on hover,
  `ACCENT` while it has the keys. The send button, 24px at the bottom right, is quiet until there
  is text; its `ACCENT` fill then eases in (and out again as the text goes), a press shrinks it a
  little in its slot and it springs back, and it sends as Enter does.

## App icon

One master, `assets/brand/logo.png`; every icon is made from it by `cargo xtask icons`, never by
hand, and committed (a test fails when they no longer match). macOS: `livtk.icns`, the art in
Apple's rounded square (824 on the 1024 grid, radius 185.4) with its soft shadow, every size from
16 to 1024; at 32 px and below a tighter crop on the smiley, so it still reads. The bundle names
it in `Info.plist`; About shows `livtk-256.png`. Windows and Linux come as more outputs of the
same generator.

## Keys and focus

- Text fields (filter, find, editor, terminal) sit outside the list's keys, as overlays or in the
  list's place, so typing never fires a shortcut. Each is known to `restore_focus`.
- Esc steps back one thing at a time: a menu, then a field or editor, then the panel, then focus
  to the list.
- J / K / Enter work the same in every list.

## Motion

Quiet and brief, and none when less motion is asked for: the highlight glides, panels and bars
slide, a new doc fades in, anchors scroll smoothly.
