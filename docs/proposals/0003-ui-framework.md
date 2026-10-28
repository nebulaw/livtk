# Proposal 0003: the UI framework and the new app

| | |
|---|---|
| **Status** | Accepted by the human, 2026-10-28; tracked as a livtk project (Liv) |
| **Written** | 2026-10-28, by Claude, from the human's brief of the same day |
| **Part of** | Proposal 0002, step 2 of §10 (the UI comes first) |
| **Folds in** | TAS-126 (dark, light and system themes) |
| **Asks for** | The decisions in §8, then an acknowledgement of the milestones in §7 |

## 1. The brief

> What matters the most is the experience and the UI first: backend and frontend will scale
> together nicely too.

The human's choices (2026-10-28):
- **The feel:** Raycast, Things 3, and the BDE MMI Spark identity by tomthings.fr.
- **The way:** a fresh app from scratch, not a port of the old one.

What each reference brings:

- **Raycast:** keyboard first. One command bar reaches everything. Every action shows its shortcut;
  an action bar keeps the hidden ones visible (their 2022 refresh). Speed is the feature.
- **Things 3:** calm. Generous space, quiet chrome, one way to create (the Magic Plus), type
  anywhere to jump ("Type Travel"), motion that is gentle and brief.
- **BDE MMI Spark:** warmth and one identity. Deep brown instead of black, warm accents, one mark
  you recognize before reading, and every surface "looking like it comes from the same place".

## 2. The livtk feel: principles, each checkable

From the research (sources in §9). Each has a target the refuter or a test can check.

| # | Principle | Target | Checked by |
|---|---|---|---|
| 1 | **Instant** | ≥95% of interactions draw their result in <100 ms, goal <50 ms; local changes show before any I/O; no spinner on a local action | input-to-frame timing in debug builds; a test per action that the view changes in the same frame |
| 2 | **Still when idle** | no frame drawn, no work done while nothing changes (DEC-6, DEC-8) | `an_idle_view_draws_nothing`, energy gate |
| 3 | **Keyboard reaches everything** | every action has a command in the command bar, with its shortcut shown; every button shows its shortcut on hover | a test that every action is registered as a command |
| 4 | **One primary action per view** | at most one accent-coloured control in a view; chrome lower in contrast than content | review against the component rules |
| 5 | **Motion only for a change** | small parts 100–200 ms, panels 200–300 ms, never >400 ms; exits shorter than entries; Reduce Motion → a fade or nothing | durations are tokens; a test that Reduce Motion requests no frames |
| 6 | **Readable and dense** | macOS scale: body 13, secondary 11–12, titles 15/17/22; rows 36, controls 28; a 4 px grid | tokens only, no raw sizes in views |
| 7 | **Contrast** | every text token on every surface ≥4.5:1 (WCAG) in light and dark | a contrast test over the token set |
| 8 | **Calm** | one count, on the inbox only; nothing moves on its own; system notifications only for what waits on the human | review |
| 9 | **Empty states teach** | one sentence, the one action and its shortcut | review |
| 10 | **One identity** | every surface uses the same tokens, the mark, the warm neutrals; nothing hand-coloured | no colour literals outside `livtk-ui` (a grep in the check) |

Anti-patterns to refuse: chrome competing with content, novelty with a learning tax, hidden
power, decorative or endless motion, spinners on local actions, made-up words.

## 3. The look

- **Warm neutrals.** Gray steps tinted warm (brown-gray), dark mode on deep brown, not black;
  light mode on warm off-white.
- **One warm accent** for the one primary action, selection and focus: an ember orange. Status
  colours (info, success, warning, danger) are separate scales, used only for state.
- **Scales, not picked colours.** Twelve steps per scale in the Radix sense (1–2 backgrounds,
  3–5 element states, 6–8 borders, 9–10 solid, 11–12 text), generated for light and dark.
  Steps 11 and 12 meet the contrast target on steps 1–2 by construction.
- **Type.** One family with a display cut for headings: the system font (SF Pro / SF Pro
  Display) by default, as native apps do. Inter is the alternative (§8).
- **The mark.** livtk's logo is the recognizable element, in the sidebar's head and the empty
  states, never decoration elsewhere.

## 4. The framework: `livtk-ui`

A crate that knows nothing of tasks. It builds on **gpui-component**, which already provides the
theme runtime, light/dark switching, inputs, virtualised lists, menus, popovers, dialogs, tooltips
and notifications. It is wrapped, so views never import gpui-component directly and its weekly
API changes stop at one crate. The API conventions follow Zed's `ui` crate (structure only:
Zed's crates are GPL).

```
crates/livtk-ui/src/
  lib.rs           init(cx), the prelude
  prelude.rs       gpui's prelude, the traits, the components, ActiveUi
  theme/
    scale.rs       12-step scales (warm gray, ember, blue, green, amber, red), light and dark
    tokens.rs      semantic tokens; spacing, text sizes, radius, elevation, motion
    theme.rs       Theme { name, appearance, tokens }; ThemeChoice { Light, Dark, System };
                   cx.ui() reads it; follows the system appearance when System
    bridge.rs      projects the tokens onto gpui-component's theme, so its widgets match
  traits.rs        Clickable, Toggleable; re-exports Selectable, Disableable, Sizable
  components/      one file each: a RenderOnce builder, tokens only
  actions.rs       shared action names (Up, Down, Open, Dismiss, Confirm); no key bindings
  preview.rs       trait Preview, and all(): every component's examples
examples/gallery.rs
```

**Semantic tokens** (about 30): surfaces (`app`, `panel`, `surface`, `elevated`, `hover`, `active`,
`selected`), borders (`subtle`, `default`, `strong`, `focus`), text (`primary`, `secondary`,
`muted`, `disabled`, `on_accent`), accent (`solid`, `solid_hover`, `subtle`, `text`), status
(`info`, `success`, `warning`, `danger` × fg, bg, border), `overlay`, `selection`.
Spacing 2/4/6/8/12/16/24; text 11/12/13/15/17/22; radius 4/6/8/full; elevation flat, popover,
modal; row 36, control 28; motion fast 120, medium 200, slow 280 ms, zero under Reduce Motion.

**Components, v1** (only what the app's views need):
- **Layout:** `Stack`, `Divider`, `Panel`, `SidebarItem`, `SectionHeader`.
- **Text:** `Label`, `Headline`, `Kbd`.
- **Controls:** `Button` and `IconButton` (ghost, subtle, solid), `Toggle`, `Chip`.
- **Lists:** `ListRow` (leading, title, meta, trailing), `GroupHeader`, `EmptyState`.
- **Status:** `StatusIcon` (by a generic `Tone`; the app maps a task state to a tone), `CountBadge`, `Avatar`.
- **Pickers:** `Menu`, `DatePicker`, `CommandBar`, `ActionBar`.
- **Feedback:** `Callout`, `Notice`.
- **Wrapped from gpui-component:** `TextField`, `TextArea`, `VirtualList`, `Tooltip`, `Dialog`, `Popover`.

**The gallery.** `cargo run -p livtk-ui --example gallery` opens every component's examples,
grouped, with a light/dark/system switch: the place a component is seen and judged alone, by the
human, before any view uses it.

**Tests.** Behaviour only, per component, headless (click fires, disabled doesn't, a key action
selects, Reduce Motion asks for no frame), plus the contrast test over every token pair. No
tests of looks: those are judged in the gallery.

## 5. The new app: Liv, from scratch

**Liv** is the new app (the human's name for it, 2026-10-28). It never
touches the app in use: the current `livtk-app` stays where it is, shipping as `livtk.app`, and
the human keeps using it every day. Liv is its own crate (`crates/liv`), its own bundle
(`Liv.app`, identifier `com.github.nebulaw.liv`), its own settings and state folders,
and its own update channel, so the two install side by side and never overwrite each other. Both
read the same projects. Nothing is copied from the old app; ideas that worked are rebuilt on
`livtk-ui`. When Liv covers the core (milestone 5), the old app is deleted and Liv is the one app livtk
ships.

**The shell** (Raycast's command bar on Things' calm):

```
┌──────────┬──────────────────────────────┬──────────────┐
│ sidebar  │  the view                    │  inspector   │
│ (mark,   │  (list, board, …)            │  (the one    │
│ projects,│                              │  thing open) │
│ views)   │                              │              │
├──────────┴──────────────────────────────┴──────────────┤
│ action bar: the primary action ⏎ · actions ⌘K           │
└────────────────────────────────────────────────────────┘
```

- **Sidebar:** the mark, the projects, the views. Navigation only, dimmer than the content, can be
  hidden.
- **View:** edge to edge, one per section.
- **Inspector:** the one selected thing; closable; nothing in it navigates.
- **Command bar (⌘K):** every command, with its shortcut, fuzzy-matched; it is also where you jump
  to a task by key or title.
- **Action bar:** the view's primary action and its key, and ⌘K for the rest, as Raycast does.
- **One way to create:** ⌘N or the action bar's primary, opening the form in place (the Magic Plus).
- **Type to jump:** typing in a list filters it.

**Architecture** (0002 §5.2):
- **Stores as entities,** one per concern: `Tasks`, `Memory`, `Inbox`, `Activity`, `Settings`. Each emits typed events. The file watcher feeds the stores, never the views.
- **Views as entities,** one per section, each implementing a `Section` trait (title, icon, render, focus, actions, save).
- **`Workspace`** holds the sidebar, the sections, the inspector, the command bar and the action bar. Nothing else.
- **Keys** live in one keymap, scoped by context. No single-letter global bindings (FND-9).
- **Rendering only reads** a store (DEC-7). Changes apply to the store first, then the disk (principle 1).

**Views, in order:** Tasks (list and board) with the inspector, Inbox, Memory (lessons, decisions,
findings), Activity, Settings (with the theme switch). What 0002 §4 cuts is never built.

**Data while the backend changes:** stores read through a small `Source` trait, with a fake one
(fixtures) for tests and the gallery, and the real one over today's core. When the storage and
service work of 0002 lands, only the real `Source` changes.

## 6. Performance and quality gates

- **Idle:** draws no frames and stays within the energy budget (kept from today).
- **Latency:** input-to-frame for each action under 100 ms, measured in debug builds, with a test per view.
- **Tokens only:** no colour or size literals outside `livtk-ui`, checked by grep in `xtask check`.
- **Size:** no module over about 1k lines, and no type with more than about 30 methods. The reviewer flags both.
- **Every action is a command,** checked by a test.

## 7. Milestones

Each is a task to acknowledge. Each ends with something the human can open and judge.

1. **Foundation.** `livtk-ui` with scales, tokens and a theme that switches between light, dark and system (TAS-126), plus typography, motion tokens and the bridge to gpui-component. The gallery shows the tokens: colour scales, type, spacing and motion. *Judge:* the palette and type in both appearances.
2. **Components v1** (§4), each in the gallery, with behaviour and contrast tests. *Judge:* every component alone.
3. **The shell.** Liv (`liv`) with the workspace, sidebar, inspector, command bar, action bar, keymap and `Section`, on fake data. *Judge:* the whole app's feel, with nothing real behind it.
4. **Tasks, for real.** The `Tasks` store over the real `Source`, the list and board, the inspector, and the create form. The new app becomes the one you use day to day.
5. **Inbox, Memory, Activity, Settings,** one task each. Then the old app is deleted and Liv is
   the one app livtk ships.

## 8. Decisions for the human

The plan was approved on 2026-10-28 without choosing among these, so each recommendation below is
taken as the default. Any of them can still be changed before the milestone that uses it.

1. **The accent:** ember orange (warm, close to the Spark reference), or another colour.
2. **The type:** the system font (SF Pro, native) or Inter (the same on every platform). Claude recommends the system font.
3. **The dark mode base:** deep warm brown, as the reference does, or neutral near-black.
4. **The gallery's place:** an example in `livtk-ui` (recommended), or a separate binary.
5. **The first real view after the shell:** Tasks (recommended) or Inbox.

## 9. Sources

**Feel**
- Nielsen, response times: <https://www.nngroup.com/articles/response-times-3-important-limits/>
- Superhuman, performance metrics: <https://blog.superhuman.com/performance-metrics-for-blazingly-fast-web-apps/>
- Zed, 120 fps: <https://zed.dev/blog/120fps>
- Linear, calmer interface (2026): <https://linear.app/now/behind-the-latest-design-refresh>
- Linear UI redesign: <https://linear.app/now/how-we-redesigned-the-linear-ui>
- Linear, invisible details: <https://linear.app/now/invisible-details>
- Raycast refresh (2022): <https://www.raycast.com/blog/a-fresh-look-and-feel>
- Things features: <https://culturedcode.com/things/features/>
- Apple HIG, motion: <https://developer.apple.com/design/human-interface-guidelines/motion>
- Apple HIG, sidebars: <https://developer.apple.com/design/human-interface-guidelines/sidebars>
- Material 3 motion tokens: <https://m3.material.io/styles/motion/easing-and-duration/tokens-specs>
- The Browser Company, letter to Arc members (2025): <https://browsercompany.substack.com/p/letter-to-arc-members-2025>
- BDE MMI Spark: <https://tomthings.fr/projects/bde-mmi-spark>

**Framework**
- Zed: `crates/ui`, `crates/theme`, `crates/component` in <https://github.com/zed-industries/zed> (structure only)
- Zed key bindings: <https://zed.dev/docs/key-bindings>
- gpui-component: <https://github.com/longbridge/gpui-kit>
- W3C Design Tokens format 2025.10: <https://www.designtokens.org/tr/2025.10/format/>
- Radix color scale: <https://www.radix-ui.com/colors/docs/palette-composition/understanding-the-scale>

Not verified (flagged by the research): the macOS text-style sizes and standard spacing (HIG pages
did not render), and Material's enter/exit guidance. They are confirmed before becoming tokens.
