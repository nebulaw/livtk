# The app's grammar

How livtk's app is put together, so every view reads as made by one hand. A new view is built
from these parts; one that needs something else changes this page first.

This grammar is for now. It will change to fit agentic work: views of who is working on what,
with their chats, terminals and the rest. When that comes, it changes here first.

## Three places

- **Left: where to go.** The sidebar: projects, the views, the tools. Navigation only, in
  sidebar rows (32px, rounded, 15px text). Nothing else uses the sidebar's look.
- **Middle: the thing itself, edge to edge.** One surface fills each content box: a list, the
  board, the graph, the terminal, a doc being read. No second sidebar, no columns nested inside
  it, no floating fields of its own. The middle can be split into panes (below).
- **Right: what is open.** The panel shows the one thing chosen in the focused pane: a task, a
  doc. Its properties, its links, its history. Metadata lives here, not in the middle.
  A node's detail can open three ways, as Notion opens a page (the button beside its close):
  side peek (this panel), center peek (a large card over the dimmed window; Esc or a press
  outside closes it, the list behind keeps the keys) or full page (in the list's place in the
  pane it was opened from; Esc gives the list back). The choice sticks.

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

## Settings

Settings are a place, not a modal (⌘, or the Settings row under Shortcuts): the sidebar becomes
their list of sections, with the way back at its top, and the middle shows the section, behind
a header bar; Esc goes back. Sections are yours (this app's, private) or this project's
(`.livtk/config.toml`, committed and shared), and each page says which. A project setting is
written through the core, checked, comments kept; one that does not hold says why and changes
nothing.

## Parts

- **List rows**: 36px, full width, `px_4`, hairline between rows; selected is `SELECTED`, and the
  highlight glides from row to row. A row leads with the same slot (priority or kind), then key,
  state, title, and ends with when it changed. Rows say something at a glance, never a bare name.
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

## Keys and focus

- Text fields (filter, find, editor, terminal) sit outside the list's keys, as overlays or in the
  list's place, so typing never fires a shortcut. Each is known to `restore_focus`.
- Esc steps back one thing at a time: a menu, then a field or editor, then the panel, then focus
  to the list.
- J / K / Enter work the same in every list.

## Motion

Quiet and brief, and none when less motion is asked for: the highlight glides, panels and bars
slide, a new doc fades in, anchors scroll smoothly.
