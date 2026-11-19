# Mira UI design system

The game fills the main window. Its title bar holds the session and game controls. Each tool gets its own window and a layout for its work. Use flat, edge-to-edge lists and forms, one-pixel dividers, and small, labeled controls. Reserve raised panels for overlays and content that needs a visible boundary.

## Theme

The built-in presets are shadcn Mira's `base-mira` style with the `neutral` palette. The table gives the exact OKLCH channels and alpha from the reference. Identical entries are intentional. For example, `accent`, `muted`, and `secondary` have different jobs and the same default color.

| Token | Light `oklch(…)` | Dark `oklch(…)` |
| --- | --- | --- |
| `background` | `1 0 0` | `0.145 0 0` |
| `foreground` | `0.145 0 0` | `0.985 0 0` |
| `card` | `1 0 0` | `0.205 0 0` |
| `card-foreground` | `0.145 0 0` | `0.985 0 0` |
| `popover` | `1 0 0` | `0.205 0 0` |
| `popover-foreground` | `0.145 0 0` | `0.985 0 0` |
| `primary` | `0.205 0 0` | `0.922 0 0` |
| `primary-foreground` | `0.985 0 0` | `0.205 0 0` |
| `secondary` | `0.97 0 0` | `0.269 0 0` |
| `secondary-foreground` | `0.205 0 0` | `0.985 0 0` |
| `muted` | `0.97 0 0` | `0.269 0 0` |
| `muted-foreground` | `0.556 0 0` | `0.708 0 0` |
| `accent` | `0.97 0 0` | `0.269 0 0` |
| `accent-foreground` | `0.205 0 0` | `0.985 0 0` |
| `destructive` | `0.577 0.245 27.325` | `0.704 0.191 22.216` |
| `border` | `0.922 0 0` | `1 0 0 / 10%` |
| `input` | `0.922 0 0` | `1 0 0 / 15%` |
| `ring` | `0.708 0 0` | `0.556 0 0` |
| `chart-1` | `0.87 0 0` | `0.87 0 0` |
| `chart-2` | `0.556 0 0` | `0.556 0 0` |
| `chart-3` | `0.439 0 0` | `0.439 0 0` |
| `chart-4` | `0.371 0 0` | `0.371 0 0` |
| `chart-5` | `0.269 0 0` | `0.269 0 0` |
| `sidebar` | `0.985 0 0` | `0.205 0 0` |
| `sidebar-foreground` | `0.145 0 0` | `0.985 0 0` |
| `sidebar-primary` | `0.205 0 0` | `0.488 0.243 264.376` |
| `sidebar-primary-foreground` | `0.985 0 0` | `0.985 0 0` |
| `sidebar-accent` | `0.97 0 0` | `0.269 0 0` |
| `sidebar-accent-foreground` | `0.205 0 0` | `0.985 0 0` |
| `sidebar-border` | `0.922 0 0` | `1 0 0 / 10%` |
| `sidebar-ring` | `0.708 0 0` | `0.556 0 0` |

Keep the palette, typography, sizes, and motion values in Rust in `mira_ui`. A runtime `Theme` owns the resolved values. App appearance selects Light, Dark, or the system's current preset. Each window, including its overlays, reads the same theme for a render. Switching presets repaints open windows in place while retaining focus, drafts, and scroll positions. The initial release needs the two built-in values and the existing appearance setting. Custom theme editing and loading belong to a later release.

Keep the source literals and alpha in the Rust presets. Resolve to the renderer's color space once per theme. Preserve the OKLCH conversions for opaque colors and the original alpha for borders. In a recipe, `token/30` multiplies the token's alpha by 30%. For example, dark `input/30` is white at 4.5% alpha. Apply specified OKLCH mixes before conversion and composite translucent paint over the actual underlying color.

Use `background` and `foreground` for windows, with their paired colors on cards and popovers. Use `border` for dividers and `input` for field outlines. Use `muted-foreground` for secondary text where it passes 4.5:1 against the rendered fill. Use the paired foreground where the tint brings the contrast below that threshold. Apply the same check to `destructive` text on tinted buttons. A red edge or status mark can carry the tone alongside a readable label. Use labels such as "Running," "Waiting," and "Failed" wherever the state matters. Chart and sidebar colors belong to their named components.

## Type, spacing, and shape

All dimensions are GPUI logical pixels. Bundle Inter Variable with its license and use it for titles as well as body text. Fall back to the platform font for missing glyphs. Use platform monospace for packet contents, code, and paths where column alignment matters. Enable tabular figures for counts, quantities, timers, and numeric columns.

| Role | Size / line height | Weight |
| --- | --- | --- |
| Body, rows, input values | 13 / 18 | 400 |
| Labels, buttons, selected labels | 13 / 18 | 500 |
| Window titles, section headings, dialog titles | 13 / 18 | 600 |
| Help, errors below fields, paths, timestamps | 11 / 16 | 400 |
| Column headings, status metadata, shortcuts | 11 / 16 | 500 |

Use sentence case. Keep titles, controls, and their labels left aligned except for centered button text and right-aligned numeric columns. Let labels and error text wrap in forms. In dense rows, keep the key identifier and action visible. Truncate long names or paths with an ellipsis and expose the full value to keyboard focus, pointer inspection, and accessibility.

| Metric | Value and use |
| --- | --- |
| Spacing scale | 2, 4, 8, 12, 16, 24, 32. Use 2 between stacked text lines, 4 between a label and its control, 8 between controls, 12 between fields, 16 at window content edges, and 24 between sections. |
| Default control | 28 high, 8 horizontal padding, 4 between label and icon. |
| Compact control | 24 high for toolbars and inline actions. Icon buttons have a square 24 or 28 hit target. |
| Roomy control | 32 high when a control needs more space in a form. Keep the same type size. |
| Rows | 28 high for one line, 40 minimum for two lines, 24 for column headers. Keep text and action columns aligned. |
| Toolbar and tabs | Fit tabs and their local filters in a shared 36px row when space allows. Use 28px tabs with 4px clearance above and below. Wrap toolbar groups at their boundaries. |
| Border | 1 thick. Use a shared divider where flat panels meet. |
| Radius | Base 10, from `0.625rem` at 16px. `sm` 6, `md` 8, `lg` 10, `xl` 14, `2xl` 18, `3xl` 22, `4xl` 26. Use `md` for controls and menu rows, `lg` for menus and popovers, `xl` for dialogs. Badges, radios, and switches use full rounding. |
| Elevation | Menus and popovers use the reference's outer 1px `foreground/10` edge and medium shadow. In CSS units the shadow is `0 4px 6px -1px black/10, 0 2px 4px -2px black/10`. Flat panes and rows have zero elevation. |

Set Lucide glyphs at 14px in default controls, 12px in compact controls, and 16px for status marks that need it. Use a glyph when it carries status or saves space on a repeated action, such as closing a tab, removing a list item, or opening row actions. Use a text label when space permits and the icon repeats its meaning. Icon-only buttons have a full accessible name and a tooltip. For a missing or more meaningful custom glyph, draw on a `24 × 24` SVG view box with a 2px stroke, round caps and joins, and inherited color. Keep icon assets and tinting in the library.

## Control states

Treat action priority, hover, press, selection, disabled, pending, and focus as separate inputs. An open trigger keeps a visible expanded state. A selected item keeps its indicator on hover and while pressed. Layout stays stable across states.

| Button | Rest | Hover | Press |
| --- | --- | --- | --- |
| Primary | `primary` / `primary-foreground` | `primary/80` | `primary/70` |
| Secondary | `secondary` / `secondary-foreground` | Mix `secondary` with 5% `foreground` in OKLCH | Mix with 10% |
| Outline | `border` edge, clear fill in light, `input/30` in dark | `input/50` | `input/80` |
| Ghost | Clear fill | `muted` in light, `muted/50` in dark | Mix `muted` with 8% `foreground` in OKLCH |
| Destructive | `destructive/10` light, `destructive/20` dark | `destructive/20` light, `destructive/30` dark | `destructive/30` light, `destructive/40` dark |
| Link | `primary` text | Underline with 4px offset | Underline with 2px offset |

Outline and ghost use the foreground of their containing panel. Text colors carry through hover and press except for the contrast rule above. Use one primary action per task area, such as Start in the game bar or Save in an editor. Keep Stop visible while a script runs. Use destructive styling for actions that remove data. Ordinary buttons also offset their face 1px down while held, within the same hit target. Popup triggers stay in place and use `muted` while open.

Actionable list rows, table rows, and tabs share an 8px radius. At rest, an unselected item has a clear background and a selected item has a `muted` fill. Use `accent/60` on unselected hover, and mix `accent` with 8% `foreground` on press. Mix the selected fill with 4% `foreground` on hover and with 12% on press. Apply mixes in OKLCH. Selection is conveyed by the entire rounded surface. Accent bars and tab underlines are prohibited. Give a labeled checkbox, radio, or switch hover and press feedback across its hit area. A checked option keeps its checkmark, radio dot, or switch thumb position. Toggles use a persistent fill and a check or an explicit On/Off label. Inputs use `input/40` on hover. A select uses `input/60` while held and shows its expanded state until dismissal. A slider thumb fills with `ring` during a drag.

Disabled controls render at 50% opacity once, skip normal tab stops, and block activation. Pending controls preserve the label or width, show the operation in progress, and prevent repeated submission. Show a usable value during refresh when one exists.

### Focus

Copy the generated Mira shadcn focus styles. The ring starts at the outside of the control, with zero offset. Reserve 3px of room around focusable controls where clipping could hide it. A border change replaces the existing border paint in place.

| Component | Focus treatment |
| --- | --- |
| Button, input, textarea, select, checkbox, switch | `ring` border and 2px `ring/30` outside. |
| Input group and combobox | The group draws the 2px treatment. Match the reference's focus-visible policy for input groups and focus-within policy for comboboxes. |
| Radio, toggle, badge used as a control, focused toast | `ring` border and 3px `ring/50` outside. Reserve the border on controls that rest without a visible edge. |
| Tabs | `ring` border, 3px `ring/50`, and 1px `ring` outline with zero offset. |
| Slider thumb | Existing `ring` border and 2px `ring/30`. Hover and drag have the same outer ring. |
| Sidebar action | 2px `sidebar-ring` outside. |
| Scroll viewport and splitter | Viewport uses 3px `ring/50` when independently focusable. Splitter uses 1px `ring`. |
| Menu, select, and combobox items | Shadcn's highlighted-row `accent` fill and `accent-foreground` text. Keep the checked mark visible. |

For an invalid standard field use `destructive` border and `destructive/20` ring in light, `destructive/50` border and `destructive/40` ring in dark. Keep the component's ring width. Input groups and other composites follow their own reference styles. A focused destructive button uses a `destructive/40` border and a 2px destructive ring at 20% in light and 40% in dark. Place one ring on the composite boundary if a labeled choice wraps the control and shares its hit target.

## Window layouts

Keep the native frame, traffic lights at `(12, 12)`, the 38px title bar, its 78px traffic-light reserve in windowed mode, and the existing fullscreen inset. Keep empty title-bar space draggable, with native double-click behavior. Controls take their own input. Preserve each window's resize, close, minimize, and fullscreen rules and the native app menus. Draw the bar with `background` and a bottom `border`. Give every window its current title in the native accessibility tree.

Mira has one game window and one live window per tool. Opening a tool again activates it, with its state and pin intact. A command with an explicit session can retarget that window. Session-scoped tools show the resolved session beside the window title, with a menu for **Follow selected tab** or pinning to a named session. Mark a pinned target with the pin glyph and an accessible Pinned state. When that session closes, return to following and show the resolved target before any further action. With zero sessions, show "No tabs open" in the body and clear the target from action controls.

### Game window

```text
┌─────────────────────────────────────────────────────────────────────────────────────┐
│ macOS   Sessions  New tab  Grid │ Windows  Scripts  Options │ Game actions    Start │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                    AQW viewport                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

Open with the rail closed. The Sessions menu shows the current name, makes other live sessions reachable, and links to Accounts. Keep New tab and Grid beside it. At narrow widths, shorten the displayed session name first and move Bank, Jump, and the automation configuration triggers into More as needed. Keep Windows, the current session, and the active script action directly reachable. Use the existing View command to show the optional 224px session rail for sustained multi-account work. Give it the `sidebar` palette and a shared edge with the game. Sessions are visible there in the same order, with one line of status where useful. The menu supplies the same commands when the rail is closed. Preserve tab shortcuts, reorder and close actions, single and grid play, and game focus.

The canvas meets the bar and fills the remaining space. In grid mode, separate viewports with 1px dividers and mark the active tile along its inside edge with a 2px `ring` rule. Keep session names available alongside any state indicator, including for keyboard and assistive technology. With no game session, give Open accounts and New tab space in a plain empty state. Game startup, login, and reconnect failures should read at the point of the affected session.

### Tools

Use a fixed title bar, a local toolbar or tab strip where the work calls for one, and a flexible body. Let a compact toolbar and its contents share the title bar's divider. Place actions that commit the whole form together at an edge of the editor. A fixed footer has a top divider, 12px vertical padding, and 16px side padding. Each dense list gets its own scroll region. When a list and detail reflow to one pane, keep the focused pane visible. Back restores focus to the selected row and restores the list's scroll position.

| Window | Layout and actions |
| --- | --- |
| Accounts | Full-width account table. Use a scope picker for All accounts and named setups next to Search. Put setup management in that scope's actions. Show account, connection, and script state in the row. Checkboxes expose multi-selection. The toolbar shows the selection count and its bulk actions in the same space as the default actions. Keep Add account and edit flows in this window. |
| Scripts | Keep Library and Queue at the left of the toolbar and align the Library search to the right. Put the Library source picker and Packages at the left of the window footer. Keep run status beside Add to queue and Run or Stop at the right. At 704px window width use a 248px result list and a flexible detail pane. Below 704px, show the selected script in the same body with Back to scripts. Put queue reordering on each queue row. |
| Combat profiles | Keep a 224px searchable profile list with New profile aligned at the leading edge. Use ordered lists for Triggers, Priority, and Rotation, with an add action beside each heading. Show the skill and its rule on the row. At 880px and above, edit the selected rule in a 280px trailing inspector. Stack the inspector below at smaller widths. Edit message matching for triggers and conditions and pauses for steps. Give Rotation a default cooldown policy and allow per-step overrides. Keep unsaved state, Revert, and Save beside the profile name. Confirm discarding drafts if closing or removing a profile would lose edits. |
| Environment | From 560px, put Drops in the left pane and stack Quests above Boosts in the right pane. Stack all three in that order at narrower widths. All three have a switch, an add field, editable chips, and a clear-list action. In Drops, show the four pickup choices in a matrix with AC and Non-AC columns and All players and Members only rows. Give each checkbox a name that includes both labels. Place Reject other drops below the matrix. Keep reward IDs and show Rewards and Requirements side by side under Pick up quest drops. Put Fetch and its status in the Boosts heading and keep first-listed priority visible. Show Copy to all tabs and Clear all lists as buttons in a footer with 8px vertical padding. State the scope in each confirmation. |
| Follower | Show Follow and Fight as two columns at 560px and above, stacked below that width. Follow contains Player, Copy walk, and ordered Search maps used when goto is refused. Search maps accept room numbers. Fight enables Profile and ordered Attack priority, which accepts names or IDs such as `id:3`. Display both lists as sortable, removable chips. Place following, travel, and refusal messages beside the Follow switch or directly below the heading. |
| Loader/grabber | Keep the source picker, search, and load action in the toolbar. For Shop, place Shop ID and a split Load control beside the source picker and search at the trailing edge. The chevron opens Load as hair shop, Open armor customizer, and Warn before loading. Shop and hair shop loads use the ID and honor the warning preference. Give the result count, loading message, and errors the same single-line space beside Load. Keep the toolbar, results, and inspector in place while refreshing and retain previous results on failure. Show a sortable results table with a 320px trailing inspector at 824px window width and above. At smaller widths, inspection opens a detail view in the same window with Back to results. Place Grab beside the selected item's title and Copy ID beside the ID itself. |
| Packets | Share a toolbar between Log and Send tabs and the Log controls. Show Log as a chronological feed of timestamps, directions, and payloads, with a 340px trailing inspector at 760px and above and a stacked inspector below that width. Preserve selection and reading position while new lines arrive. Keep capture status beside Pause or Resume. Send puts the composer and queue side by side at 784px and stacks them below. Use the Send tab for the queue count and the footer for Run queue and any send status. |
| Settings | Use General, Appearance, Updates, and Shortcuts tabs. Build each preference section with `Group` and `FieldRow`. In Updates, put Check for updates and its result in App updates and the GitHub access token in Script packages. Size shortcut capture to its keycaps and hit target. Keep conflicts and Reset beside that control. |

Use the same dialog for app and script prompts, with the originating script and session legible when the prompt belongs to one. Leave in-game AQW interface elements to the player.

## Components and placement

Layout belongs to the consuming view. Components own their internal spacing, states, hit areas, and focus behavior. Expose width constraints, alignment, and supported orientation through the public Rust API. Support leading and trailing regions for buttons and fields, above or beside for field labels, either edge for split panels, and horizontal or vertical tabs, separators, and sliders. Layout changes must retain stable control identities and match the keyboard traversal to the visible order.

| Component | Appearance and use |
| --- | --- |
| Toolbar, menubar, separator | Group related actions with 4px between controls and 8px around short separators. Use menus for commands and popovers for forms. Move a group to More before its labels crowd. Keep any open menu anchored to a visible trigger. |
| Button, toggle, toggle group | Use buttons for commands and toggles for persistent on/off state. Join a default command to a chevron when its menu contains closely related variants. Give each half its own accessible name and focus. Segment a short group when its members change the same setting. Use a 4px gutter between independent controls so their rings stay readable. |
| Field, input, textarea, shortcut capture | Fields have a persistent label and optional help and error below. Inputs use `input` border and `input/20` fill in light or `input/30` in dark. Give multiline fields at least 64px height. Keep a reveal, clear, or reset button at the field's trailing edge if needed. Shortcut capture hugs the keycaps with 4px side padding and at least a 24px hit target. |
| Checkbox, radio, switch, slider | Use 16px checkbox and radio marks with `primary` fill and `primary-foreground` indicator when checked. Give the checkbox a 4px radius. A switch has a 28 × 16.6px track and 14px thumb. Its track uses `primary` when on and `input` when off in light, `input/80` when off in dark. Use a `background` thumb in light and `primary-foreground` on or `foreground` off in dark. Use a 4px `muted` slider track with `primary` fill and a 12px thumb. Give compact marks at least a 24px hit area through their labeled row. |
| Select, combobox | Use Select for a short list of known values and searchable Combobox for long lists or filtering. Keep the committed selection visible on the trigger. Show a checkmark on the selected option and highlight the currently navigated option separately. |
| Tabs | Use rounded `muted` fill for the selected tab. Give separate close and row actions their own space and hit areas. Scroll or use overflow for long strips. |
| List, table, tree, sortable rows | Use flat rows with a header only when it describes columns or groups. Let the key column stretch. Keep numbers and row actions aligned. Use a tree for real hierarchy. Overlay the dragged row and show the proposed insertion with a 2px rule. Provide Move up and Move down in the row's actions. |
| Scroll area, splitter | Use a 10px scrollbar lane with a rounded thumb and a clear hover or grab state. Allow the focus ring to clear the scroll clip. Use a 1px splitter with an 8px drag area only where resizing panes helps the work. |
| Card, menu, context menu, submenu, popover | Use a flat `card` with the outer 1px `foreground/10` edge, `lg` radius, and 16px padding when a group needs a boundary. Menus have 4px padding, 28px rows, and a trailing column for shortcuts or submenu indicators. Popovers use 10px padding and contain a small task with controls or inputs. |
| Dialog, alert dialog | Use a centered `popover` panel with `xl` radius, 16px padding, an outer 1px `foreground/10` edge, and a black/80 backdrop on the body. Default width is 384px, capped by the usable viewport with at least 16px per side. Scroll the body at short heights. Keep title, Cancel, and commit actions reachable. |
| Tooltip, hover card | Tooltips use `foreground` fill and `background` text at 11px with 6px vertical and 12px horizontal padding. Use a hover card for a read-only preview and provide the essential information in the window. Put interactive content in a popover. |
| Badge, key hint, status, empty state, alert | Use 11px text and a `secondary` fill for meaningful counts. Key hints use `muted` fill and the current binding. Use unfilled status text for script and connection states. Reserve alerts for a condition that needs attention at its source. Empty states name what's missing and show a direct action when one exists. Distinguish empty data, filtered results, loading, and failure. |
| Progress, spinner | Use a 4px track with a `primary` indicator when measurable and a compact spinner with a readable label when indeterminate. Keep failure and completion visible at the originating control or section. |

### Anchoring

For anchored overlays, accept an element's measured bounds or a point, a preferred side, start/center/end alignment, and side and alignment offsets. Support top, right, bottom, and left. Use live coordinates in the owning window. Alignments are relative to the anchor along the adjoining edge. Dialogs center in the usable body. Toasts use a window corner.

| Overlay | Default placement |
| --- | --- |
| Dropdown menu | Below, start aligned, 4px gap. End align a trailing row menu. |
| Submenu | Right, start aligned, 0px gap with a −3px alignment offset. Flip to the left when needed. |
| Select / combobox | Below, at least the trigger width. Select can align its selected row with the trigger when both fit. Use a 4px gap when opening below or above. |
| Context menu | Start at the mouse point or the focused row's bounds for keyboard invocation. Keep the initial pointer clear of the first action. |
| Popover / hover card | Below, center aligned, 4px gap. Apply a 4px alignment offset to the hover card. The caller can align to a field or edge. |
| Tooltip | Above, centered, 4px gap. Flip below when blocked by chrome. |

Fit within the window body with an 8px gutter. Try the preferred side, then its opposite. Shift along the alignment axis to stay inside the gutter. Increase the gap as needed for title-bar triggers. Constrain width and height and scroll oversized content. Keep a submenu beside its parent row as space allows and use another side when neither side has room. Derive the transform origin from the **resolved** side and the visible point nearest the anchor. Recompute on scroll, resize, or content changes. Dismiss when an element anchor leaves the visible region or disappears.

Render overlays above scroll clips. Paint transient notifications above content, then anchored overlays, then modal backdrops and dialogs. An overlay opened within a dialog layers above its owning dialog.

## Keyboard and accessibility

Model Base UI's roles, selection, and focus lifecycles with GPUI and macOS accessibility. Publish each control's name, role, value and relevant selected, checked, expanded, invalid, or disabled state. Associate help and errors with their control. Tell assistive technology when focus moves or a value changes. Coalesce frequent status updates so a running script or packet stream stays usable.

Tab and Shift+Tab follow the visible order and stop once per composite. Arrow keys move the active item inside a composite and scroll it into view. Keyboard focus shows the reference treatment. A clicked text input also shows focus for editing. Preserve indicated selection separately from keyboard focus. Restore the last focused element on closing an overlay when it is still usable. If it vanished, move to the next meaningful control in its group or the group's trigger.

| Control | Keyboard contract |
| --- | --- |
| Button, toggle, checkbox, switch | Space activates on release. Return activates a button. Click or press only commits once. |
| Toolbar and tabs | Arrows follow the layout's axis. Home and End jump to the ends. Tabs use manual activation with Return or Space. A local panel that changes immediately can opt into activation on focus. A game-session tab always requires explicit activation. |
| Radio group and slider | Arrows change the radio choice or step the slider. Home and End move the slider to the bounds. Expose the slider's current value, minimum, and maximum. |
| Menu and submenu | Return, Space, or Down opens from the trigger. Up opens at the last item. Up/Down, Home/End, and typeahead navigate. Return or Space activates the item. Right opens a submenu and Left returns to its trigger. Escape dismisses the current level. Tab exits the menu and continues the window's tab order. Arrow navigation includes disabled items so users can discover their state. |
| Select and combobox | Select opens at the committed option or first available option. Arrows and typeahead navigate. Return or Space commits in Select. In Combobox, keep text editing focus while arrows move the highlighted result and Return commits it. Escape closes and retains the previous committed value. |
| Lists, tables, trees | Arrow through selectable rows within a single tab stop. Expose named headers and the sort direction. A tree adds Right to expand or enter and Left to collapse or return to a parent. Shift+F10 opens the focused row's context menu. A read-only table exposes its structure and copyable values. |
| Popover and dialog | Move keyboard focus to the first relevant control when opening an interactive panel. Only a modal traps Tab. Initial focus is the input for a prompt and Cancel for a destructive confirmation. Escape closes the top layer or cancels its task. Explicit Cancel or confirmation is required for a destructive alert or unsaved-edits dialog. |
| Tooltip and hover card | Keep focus on the trigger, open on keyboard focus, and allow Escape to dismiss. Keep a pointer-triggered popup available while the pointer is over the trigger or its content. |
| Shortcut capture | Capture only while editing the field. Escape cancels capture. Report conflicts inline and keep the committed shortcut until a valid one replaces it. |

Outside press dismisses a menu or nonmodal popover. Honor focus on the clicked control when it legitimately changes. Consume a dismissing click over the game canvas. Native input editing and IME composition take precedence over window shortcuts. When an overlay owns input, Escape and Tab belong to its focus scope. When the canvas owns input, keep game keys in the player and preserve the existing app shortcuts for switching contexts.

## Motion and notifications

Paint hover, selection, and keyboard navigation immediately. For pointer-opened menus and popovers, move from 95% scale at the resolved anchor and fade in over 100ms with ease-out. Dialogs use the same duration from the center. Keep the window's layout still during transitions. Delay the first toolbar tooltip by 500ms. Adjacent tooltips open immediately while the group is active. Reduced Motion skips transitions.

Put a durable message beside the action or data it describes. Use unfilled 11px text for normal state and pending work, with a spinner for indeterminate progress. Use a 13px message and optional 11px detail for interruptions. A destructive-color icon can signal severity while the text retains foreground contrast. Keep the affected data visible when possible, and place any recovery action with the originating control. Present changes with status semantics and reserve alert semantics for interruptions that need attention. A working script reports beside its Run or Stop control. A blocked follow reports beside the target. The game reports reconnects within the affected session viewport.

Use shadcn toast paint and Sonner motion. Toasts belong to the window where the action happened. Default to bottom-end, 16px from the body edges, with a width up to 360px. Use `popover` and `popover-foreground` with `border`, `md` radius, and 16px padding. The large shadow in CSS units is `0 10px 15px -3px black/10, 0 4px 6px -4px black/10`. Keep a brief title, optional description and action, and a named dismiss button. Field errors stay in the form. Use a toast for a completed background action or an outcome that needs a short message away from its source.

- Show up to three cards, newest in front. Behind the front card, show only card edges. Each depth peeks by 14px and scales down by five percentage points. Expand on hover or toast focus, with 14px between the measured cards.
- Enter from the anchoring edge by one card height and fade from zero opacity. The front card leaves toward the same edge by one height and fades out. For a bottom stack, a covered card that exits while collapsed drops 40% of its height and fades. Mirror vertical directions for a top stack.
- Enter, exit, and reposition with 400ms `ease` (`0.25, 0.1, 0.25, 1`). Covered-card exit uses 500ms transform and 200ms opacity. Retarget from the current position and opacity when another toast arrives, closes, or expands the stack. Retain the exiting card for its animation, with actions and hit testing removed. Animate the overlay without relaying out the window or ticking the game.
- Time ordinary notices for 4 seconds in front. Pause while covered, hovered, or focused, or while the window is inactive or blocked by a modal. Update the existing card when the same operation reports a new state. Keep an action that requires a response available until handled or dismissed. On Reduced Motion, apply final positions and opacity immediately.
- Announce additions once as status. Use an alert for an urgent error. F6 moves focus to Notifications in app UI. Provide a native menu command to reach it while the canvas has focus. F6 again or Escape returns to the previous focus. Opening a toast never moves focus. When a focused card closes, move to the next card or return to the prior control. Covered cards have no interactive targets while collapsed.

## Acceptance

- Compare light and dark tokens against the reference literals, including alpha and all radius multipliers. Compare focus samples with the corresponding generated shadcn component.
- Render component states in both presets with `mira_ui::snapshot`. Include primary, hover, pressed, selected, disabled, invalid, menu highlight, and keyboard focus. Check contrast after blending, long labels, a narrow window, and focus near a scroll edge.
- Render every tool at its default size and at its narrowest supported layout. Check long session names, non-empty and empty results, and persistent error or pending states. Check that native chrome and the title-bar hit regions stay usable.
- Exercise keyboard traversal, submenu dismissal, focus restoration after deletion and resize, form errors, and notification timing with Reduced Motion. Check that opening a tool twice raises the same window and that closing its pinned session resolves to the selected one.

## Design canvas

The GPUI storyboard arranges the game and eight tools in rows on a pannable canvas. Align frame origins within each row and column. Put the name and logical size above the frame so it can be identified at overview zoom. Fit leaves room below the frames for the zoom controls.

Click a frame or use the Windows menu to inspect the same GPUI view at its normal type size. The overview shows the starting state. Keep edits in the inspected views until the canvas closes. Use preview fixtures for game, account, and automation actions. The inspection chrome has a Statuses menu with Current, In progress, and Needs attention where applicable. Keep these examples in the affected tool's normal layout. Show sample toast uses the same notification layer as actions performed in the preview.

## Local references

The reference checkouts live in `~/dump/mira-research`.

- `shadcn-mira/src/index.css` and `components.json` define the preset. `src/components/ui/` contains the complete `base-mira` registry generated by shadcn CLI 4.23.0.
- `base-ui` at `f0061cb342ed2e54cdf2c50823c93077782d1a16` models keyboard behavior, accessible state, focus lifecycles, and anchor collision.
- `sonner` at `8e4662b39255120b62138312058f5d77c0139a5e` defines the stack motion in `src/styles.css` and timer behavior in `src/index.tsx`.
