# The app's grammar

How livtk's app is put together, so every view reads as made by one hand. A new view is built
from these parts; one that needs something else changes this page first.

## Three places

- **Left: where to go.** The sidebar: projects, the views, the tools. Navigation only, in
  sidebar rows (32px, rounded, 15px text). Nothing else uses the sidebar's look.
- **Middle: the thing itself, edge to edge.** One surface fills the content box: a list, the
  board, the graph, the terminal, a doc being read. No second sidebar, no columns nested inside
  it, no floating fields of its own.
- **Right: what is open.** The panel shows the one thing chosen in the middle: a task, a doc. Its
  properties, its links, its history. Metadata lives here, not in the middle.

The flow is always the same: a list in the middle → open one → its details in the panel. A
thing that wants the whole middle (the terminal, a doc being read, a doc being edited) takes the
list's place, behind a header bar, and Esc gives the list back.

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
