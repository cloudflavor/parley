# TUI High-End Upgrade Roadmap

## Objective

Upgrade the Parley TUI from feature-rich to high-end by improving interaction quality, information hierarchy, navigation ergonomics, rendering performance, and visual coherence.

This document defines implementation scope for all seven proposed upgrades and breaks work into small, independent units.

## Design principles

- Keep keyboard-first workflows primary.
- Preserve existing review/thread state semantics.
- Make rendering deterministic and low-latency.
- Prefer additive, reversible changes over large rewrites.
- Keep each milestone shippable on its own.

## Constraints

- Existing stack: `ratatui = 0.29`, `crossterm = 0.29`.
- Current TUI architecture is immediate-mode rendering with centralized `TuiApp` state and input handling.
- No changes to review persistence format unless explicitly required.

## Baseline issues to address

- Diff viewport is coupled to selection-based autoscroll, creating jumpy navigation.
- Status panel is overloaded and consumes vertical space with persistent cheat sheets.
- Keybindings are powerful but hard to discover/remember under load.
- File navigation is flat and scales poorly with large diffs.
- Threads are always expanded and visually heavy.
- Per-frame rendering/wrapping work is high for large diffs.
- Syntax highlight palette is not harmonized with theme palette.

## Implementation status (2026-04-17)

### Completed

- Workstream 1 (cursor/viewport decoupling)
  - Independent pane viewport state (`primary_viewport_top_row`, `secondary_viewport_top_row`)
  - Stable paging semantics (`PageUp/PageDown`, `Ctrl+U/Ctrl+D`) and `zz` center behavior
- Workstream 2 (status simplification + contextual hints)
  - Compact status panel with contextual hints and `?` help modal
- Workstream 3 (command palette)
  - `Ctrl+K` palette with searchable actions for thread/review/layout/AI/refresh
- Workstream 5 (thread density controls)
  - Compact/expanded density mode, selected-thread focus behavior, per-thread toggle
- Workstream 7 (visual-system coherence)
  - Semantic token mapping layer in `syntax.rs`
  - Theme picker metadata (family/variant) and preview swatches

### Partially completed

- Workstream 4 (file navigator upgrade)
  - Done: grouped directory view, collapse/expand, filter modes, sort modes, file filter search
  - Updated UX decision: filter/sort remain keyboard-first (`Shift+F`, `Shift+O`), and the visual tabs row is removed
  - Updated UX decision: files-pane visual scrollbar is removed

### Completed in this pass

- Workstream 6 (render loop + caching performance)
  - Added scoped diff cache invalidation (`clear_diff_render_cache_for_file`) for file-local changes
  - Reduced unnecessary cache clears for search query state transitions (cache key separation handles correctness)
  - Added unit test coverage for scoped cache invalidation behavior

### Remaining implementation items

- Run and record manual verification checklist for all milestones:
  - keyboard-only navigation
  - split pane behavior
  - resize behavior
  - search/thread jump behavior
  - AI progress popup behavior
  - theme switching

### Newly completed in this pass

- Focused TUI state-transition tests added in `src/tui/app/state.rs`:
  - file filter/sort/search visibility behavior
  - selection constraint under file filtering
  - collapse-all-visible-groups scoping
  - redraw invalidation roundtrip behavior

## Workstream 1: Cursor/Viewport Decoupling

### Goal

Make navigation feel precise and stable by separating cursor movement from viewport scrolling.

### Implementation

- Add per-pane viewport state:
  - `primary_viewport_top_row`, `secondary_viewport_top_row`
  - keep existing selected line fields as cursor row.
- Replace selection-driven `compute_scroll` usage with explicit viewport state.
- Add page semantics:
  - `PageUp/PageDown`: full page scroll.
  - `Ctrl+U/Ctrl+D`: half page scroll.
  - `zz`: center cursor in viewport.
- Keep cursor in bounds while preserving viewport when possible.

### File targets

- `src/tui/app.rs`
- `src/tui/app/state.rs`
- `src/tui/app/input.rs`
- `src/tui/app/render.rs`

### Acceptance criteria

- Moving cursor line-by-line does not recenter/jump viewport unexpectedly.
- Paging operations move viewport predictably and preserve cursor intent.
- Split panes maintain independent viewport positions.

## Workstream 2: Status Panel Simplification + Contextual Hints

### Goal

Reduce cognitive load and recover vertical space while preserving operator awareness.

### Implementation

- Replace long static help lines with compact, mode-aware hints.
- Status panel layout:
  - line 1: mode/state chips and current file/thread summary.
  - line 2: transient status message + minimal hints relevant to current mode.
- Add toggle for extended hints (`?` opens full help modal).
- Keep version display, but avoid constant keywall rendering.

### File targets

- `src/tui/app/render.rs`
- `src/tui/app/input.rs`
- `src/tui/app.rs`

### Acceptance criteria

- Status panel is readable within 2 lines at standard terminal heights.
- Full key reference remains available via modal.
- Mode transitions update hints correctly.

## Workstream 3: Command Palette (Action Model)

### Goal

Expose power features through searchable actions instead of memorized single keys only.

### Implementation

- Introduce action registry:
  - action id
  - label
  - optional shortcut hint
  - execute function
- Add `Ctrl+K` command palette popup with incremental filtering.
- Include at minimum:
  - thread navigation actions
  - state transitions
  - layout toggles
  - AI run actions
  - refresh/reload actions
- Reuse existing command prompt text input behavior where possible.

### File targets

- `src/tui/app.rs`
- `src/tui/app/state.rs`
- `src/tui/app/input.rs`
- `src/tui/app/render.rs`

### Acceptance criteria

- Operator can execute core actions without knowing single-key bindings.
- Palette supports keyboard-only selection/execute/cancel.
- Existing keybindings continue to work.

## Workstream 4: File Navigator Upgrade

### Goal

Make file navigation scalable for large, multi-directory diffs.

### Implementation

- Build grouped navigator entries by directory path.
- Support collapse/expand per directory group.
- Add filter modes:
  - all files
  - files with open threads
  - files with pending threads
- Add sort modes:
  - path
  - open-thread count desc
  - total-thread count desc
- Keep existing comment markers, improve density/readability.
- Add visual scrollbar for long file lists.

### File targets

- `src/tui/app/state.rs`
- `src/tui/app/input.rs`
- `src/tui/app/render.rs`

### Acceptance criteria

- Large file sets remain navigable without excessive scrolling.
- Filter/sort changes are immediate and deterministic.
- Grouping does not break existing file selection semantics.

## Workstream 5: Thread Density Controls

### Goal

Reduce visual noise and show detail on demand.

### Implementation

- Add thread display modes:
  - compact (default): one-line preview per comment/reply block.
  - expanded: full markdown body rendering.
- Auto-expand selected thread, keep others compact unless toggled.
- Add per-thread expand/collapse toggle key.
- Keep anchor/line metadata visible in compact mode.

### File targets

- `src/tui/app/state.rs`
- `src/tui/app/input.rs`
- `src/tui/app/render.rs`

### Acceptance criteria

- Default diff view shows materially more code context.
- Selected thread remains easy to inspect/edit.
- Thread actions still map correctly in both modes.

## Workstream 6: Render Loop + Caching Performance

### Goal

Improve responsiveness on large diffs and reduce unnecessary redraw cost.

### Implementation

- Introduce explicit invalidation/dirty flags for redraw decisions.
- Separate event-driven redraw from periodic animation ticks.
- Cache wrapped/render-ready lines by stable key:
  - file index/path
  - pane width
  - view mode (unified/side-by-side)
  - search query
  - thread density mode
  - selected line for style overlays where applicable
- Keep existing syntax row cache; avoid duplicate wrap work.

### File targets

- `src/tui/app.rs`
- `src/tui/app/state.rs`
- `src/tui/app/render.rs`

### Acceptance criteria

- Reduced perceived latency when scrolling/searching in large files.
- No stale rendering artifacts after resize/theme/search updates.
- AI spinner/progress still updates smoothly.

## Workstream 7: Visual-System Coherence

### Goal

Make themes feel intentional and consistent across syntax, diff, and UI chrome.

### Implementation

- Add semantic token mapping layer in `syntax.rs` to align syntect output with active theme palette.
- Normalize syntax contrast against diff/thread backgrounds.
- Expand theme schema if needed for additional semantic roles (keep backward compatibility by defaults).
- Add preview metadata in theme picker (light/dark family + sample swatch line).

### File targets

- `src/tui/theme.rs`
- `src/tui/syntax.rs`
- `src/tui/app/render.rs`
- `src/tui/themes/*.json` (only if new tokens are introduced)

### Acceptance criteria

- Syntax coloring remains readable in all bundled themes.
- Switching themes no longer causes strong style mismatch between syntax and UI elements.
- Theme picker communicates theme identity better than name-only list.

## Ratatui 0.29 capabilities to leverage

Verified against `ratatui v0.29.0` docs/examples:

- `Scrollbar`/`ScrollbarState` for diff, thread, and file-list scroll affordance.
- `Table` for richer file navigator rows (counts, status columns).
- `Tabs` for compact mode/filter/sort controls.
- Existing `ListState` pattern remains valid for stateful selection.

## Delivery plan (small shippable increments)

1. Milestone 1: Workstream 1 + 2
2. Milestone 2: Workstream 3
3. Milestone 3: Workstream 4 + 5
4. Milestone 4: Workstream 6
5. Milestone 5: Workstream 7

Each milestone should include:

- unit tests for new state transitions where practical
- manual TUI verification checklist
- no persistence format changes unless explicitly required

## Verification checklist

For each milestone run:

- keyboard-only navigation checks
- split pane behavior checks
- resize behavior checks
- search/thread jump checks
- AI progress popup behavior checks
- theme switch checks

### Verification status (2026-04-17)

- Automated checks completed:
  - `cargo fmt --all`
  - `cargo check -p parley`
  - `cargo clippy -p parley --all-targets --all-features -- -D warnings`
  - `cargo test -p parley`
- Manual TUI checklist execution remains pending (interactive run required).

## Risks and controls

- Risk: state complexity growth in `TuiApp`.
  - Control: isolate new state into focused structs per feature.
- Risk: render cache invalidation bugs.
  - Control: explicit cache keys + targeted invalidation paths.
- Risk: UX regression for existing users.
  - Control: preserve current keybindings and defaults where possible.

## Out of scope

- rewriting persistence model
- changing review/thread domain semantics
- adding remote/networked collaboration behavior
