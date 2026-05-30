# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

sparktop is a terminal-based system monitor written in Rust that enhances the traditional `top` utility by tracking historical CPU usage per process. This allows users to see "what caused everything to be slow 30 seconds ago" rather than just current snapshots. The application uses [ratatui](https://ratatui.rs) with the crossterm backend for TUI rendering and displays sparklines for CPU history.

## Memory System

**IMPORTANT**: Update this section frequently! After meaningful conversations or when learning new context about the user's setup, preferences, or workflow, add notes here.

**Tiered Memory Architecture:**
- This file contains **project-level** memories specific to sparktop
- Global/user-level memories live in `~/.claude/CLAUDE.md`
- **How to bubble up**: When you learn something important that applies across ALL projects:
  1. **Do it immediately** - don't wait until end of session
  2. Read `~/.claude/CLAUDE.md` to locate the "Incoming memories" section
  3. Use the Edit tool to append a new memory entry with appropriate category
  4. Format: `- YYYY-MM-DD HH:MM TZ | [category] | project: sparktop | <memory content>`
  5. Categories: `[tech-pref]`, `[workflow]`, `[tools]`, `[comm]`, `[general]`
  6. Example: `- 2025-11-22 15:30 PST | [tech-pref] | project: sparktop | User prefers TUI over GUI for system monitors`

### Recent Context & Memories

- Rust-based terminal system monitor
- Uses EWMA smoothing and ring buffers for historical tracking

### Preferences & Patterns

- Pre-commit hooks must run before committing
- **Test-Driven Development**: When implementing new features or fixing bugs, write tests first before changing implementation code

## Build and Development Commands

**Build and run:**
```bash
cargo build                    # Build the project
cargo run                      # Run with defaults
cargo run -- -d 0.5 -e 0.3     # Run with custom delay (0.5s) and EWMA weight (0.3)
```

**Testing and benchmarking:**
```bash
cargo test                     # Run all tests
cargo test <test_name>         # Run specific test
cargo bench                    # Run benchmarks (sysinfo_refresh)
```

**Code quality:**
```bash
cargo fmt                      # Format code
cargo clippy                   # Lint code
pre-commit run --all-files     # Run pre-commit hooks (fmt, cargo-check, clippy)
```

**Before committing:** Always run `pre-commit run --all-files` or let pre-commit hooks run automatically. The hooks check for large files, merge conflicts, TOML syntax, trailing whitespace, and run fmt/cargo-check/clippy.

## Recent Changes

**2026-05-30: sustained (time-weighted) CPU ranking**
- `SProc.cpu_rank`: a slow EWMA (`CPU_RANK_WEIGHT = 0.1`, ~7-tick half-life)
  alongside the display `cpu_ewma`, started at 0 so a freshly-spiking process
  ranks low and only climbs if usage persists; long-sustained load stays sticky.
  Summed in `SProc::aggregate`.
- `ViewState.sustained` (default **on**), toggled with `r` ("(r)ank" in footer);
  only affects the CPU sort. `View::rank_value`/`ranks_sustained` pick `cpu_rank`
  vs the instant metric; used by both flat `sort` and tree `own`. Display/sort
  can disagree (a just-spiked proc sits lower than a steady one) — the sparkline
  column makes that legible.
- `flat_rows` re-sorts every tick when `ranks_sustained()` (the slow metric is
  smooth, so no jitter); the freeze still applies in instant mode. `last_sort`
  gained the `sustained` bool so toggling forces a re-sort. Tests that assert
  cpu-ordering use the `instant_view()` helper.

**2026-05-30: CPU profiling pass + reversed tree + scoped flash**
- **Disabled sysinfo's `multithread` (rayon) feature** (`default-features = false`
  in Cargo.toml). Profiling showed steady-state CPU was dominated not by the
  refresh work itself (~2.6 ms) but by rayon's 8 worker threads waking and
  spin-waiting every tick. Serial refresh has higher per-call wall time (~6.8 ms,
  harmless once a second) but ~60% lower *total* CPU (~2.2% → ~0.9% at 1 Hz) and
  runs single-threaded. Measure via cumulative cputime over a window, not `ps`
  %CPU (which spikes at the tick).
- **Trimmed per-tick refresh kinds** in `SProcs::update`: `refresh_cpu_specifics`
  with `CpuRefreshKind::new().with_cpu_usage()` (skip per-core frequency, unused);
  `refresh_processes_specifics` with exactly cpu/memory/disk_usage + user/cmd
  (`OnlyIfNotSet`). The default `refresh_processes()` fetched `exe` (unused) but
  *not* user/cmd, so processes appearing after startup were missing their owner
  and full cmdline — this fixes that latent bug too.
- **Reversed tree view**: `tree_rows` mirror-reverses its DFS output so leaves sit
  at the top and roots at the bottom; the bottom-corner glyph `╰` is flipped to
  `╭` so connectors read correctly upside-down (`├`/`│` are vertically symmetric).
  Sibling ordering key is `(is_internal, by_total)`: leaf children are grouped
  directly against their parent and subtrees stack above (stops a lone leaf from
  wedging between a sibling subtree's rows — the "split branch" look), and
  `by_total` is flipped so the busiest branch lands near the top after reversal.
  Caveat: vertical mirroring inverts how indentation reads, so the boundary
  between two stacked sibling *subtrees* still shows one shallow→deep step
  (inherent; can't be removed without abandoning the reversed layout).
- **Highlights scoped to the name span**: both the amber new-row flash and the
  selection reverse-video apply only to the process name `Span` (not the dim tree
  indent, not the whole row). Dropped `Table::row_highlight_style`; `TableState`
  still drives auto-scroll. `ProcTable::build` takes a `Highlights { flash,
  selected }` bundle (keeps the arg count under clippy's threshold).
- **Aggregation groups process families**: `aggregate_key` collapses a trailing
  role parenthetical (" (Renderer)", " (GPU)") and a trailing " Helper" so the
  whole Chrome/Electron family folds into one row (e.g. "Google Chrome (12)").
  `SProc::aggregate(name, members)` now takes the canonical group name.
- **Anti-flicker grace window for hide-idle**: `View.keep_alive: HashMap<Pid,u8>`
  (reset to `KEEP_ALIVE_TICKS` whenever a proc is at/above `IDLE_CPU_PCT`, aged
  each draw via `update_keep_alive`). `is_active` = above threshold *or* still in
  the grace window, used by both the flat `visible` filter and tree
  `active_branches`. Stops processes that briefly cross the threshold from
  appearing then vanishing the next tick, and cuts visible-set churn (so
  `flat_rows` re-sorts less, steadier order).

**2026-05-30: readability pass — stable order, flash, aggregate, core history**
- **Stable list order**: `View::flat_rows` freezes the order and only re-sorts on
  sort change or visible-set change; sort gained a pid tiebreak (kills HashMap
  shuffle). Fixes the "rows jump every tick" problem.
- **Flash new rows**: rows entering the visible set get a fading amber wash
  (`note_new_rows`/`fade_flashes`, `FLASH_TICKS`).
- **Sort arrow**: header shows ▾/▴ on the active column (replaces the `*..*`).
- **Per-core history in header**: `SProcs.core_hist` + `SysSummary.cores`;
  `view::core_lines` renders compact per-core sparklines, packed to width.
- **Aggregate by name** (`a`): `SProc::aggregate` / `view::aggregate_by_name` fold
  same-named procs into one summed synthetic row (histories summed element-wise,
  id = lowest pid). Uses the flat path.

**2026-05-30: user/state columns + richer detail view**
- `SProc` gained `user`, `state` (char), `cmd`, `threads`, `run_secs` (and
  `parent` earlier). `SProc::new(p, user)` replaces the old `From<&Process>`;
  `SProcs` holds a `Users` table and resolves the owner name on insert.
- New toggleable **user** (`u`) and **state** (`e`) columns; state is color-coded
  (R green, D/Z red, X gray).
- **Detail view** header expanded to identity (pid/ppid/user/state/threads/uptime),
  a metrics line, and the full **cmdline** (wrapped). `fmt_uptime` moved to
  render.rs (shared by the summary header and detail).

**2026-05-30: Gap-analysis features (vs bottom/btop/htop/zenith)**
- **Name filter / search** (`/`): `ViewState.filter` + `filtering`; `View::visible`
  filters by name substring (takes precedence over hide_idle).
- **System summary header**: `SProcs::summary() -> SysSummary` (global cpu, mem/swap,
  load avg, uptime, task count); rendered as a heat-shaded one-line header above the
  list (`view::summary_line` / `fmt_uptime`). `update()` now also `refresh_memory`.
- **Tree view** (`t`): `View::tree_rows` builds a parent→child DFS order (depth-indented
  names) from `SProc.parent`; siblings sorted by the active key; cycle-guarded.
- **Bug fix**: memory was shown ~1000x too small (divided bytes by 1024 as if KB since
  the sysinfo 0.30 upgrade). Now stored as `mem_bytes` and rendered via `human_bytes`,
  consistent with the disk columns.

**2026-05-29: Process detail view, navigation, and list features**
- **Process detail view** (`src/detail.rs`): press `⏎` on a selected process for
  a full-screen drill-down with high-res braille line charts (ratatui `Chart`,
  `Marker::Braille`) of its cpu / memory / disk-i/o history. `esc` returns; `↑/↓`
  flip between processes. Layout is responsive: charts stack vertically, but go
  side-by-side in short, wide panes (`use_horizontal`).
- **Selection + navigation**: `ViewState.selected` (a `Pid`, stable across
  re-sorts); `View` caches display order + a `TableState` for a reversed-video
  highlight and auto-scroll.
- **Memory history**: added `SProc.mem_hist` (cpu/disk already had ring buffers).
  The disk buffers were previously tracked-but-never-rendered; the detail view
  now uses them.
- **Hide idle processes** (`i`, on by default): hides procs below `IDLE_CPU_PCT`
  (0.5%); selected proc always kept visible. Filtering happens in `View::visible`
  before sort/selection.
- **Multi-height bars** (`b`, cycles 1/2/3): `render_vec_colored_multi` draws the
  cpu sparkline across N rows where each row = 100%, so >100% usage stacks
  visibly.
- **Reversible sort**: re-selecting the active sort column flips Asc/Desc.
- **Human-readable disk columns** (`render_bytes` / `render::human_bytes`).
- Tests: `SProc::blank` test constructor; `detail` uses ratatui `TestBackend` to
  assert rendered buffer content (don't scrape terminal escapes); ignored
  `detail::tests::preview` prints sample screens (`--ignored --nocapture`).

**2026-05-29: Reverted history compression + migrated tui → ratatui**
- **Reverted the dynamic CPU-history compression feature** (it was janky). `render.rs`
  is back to the simple sparkline renderer: each sample is one bar, the table cell
  truncates to the visible width. SAMPLE_LIMIT is back to 60.
- **Migrated `tui` (deprecated, unmaintained) → `ratatui` 0.30** (its maintained
  successor). `Spans` → `Line`, `f.size()` → `f.area()`, `Table::new(rows, widths)`,
  crossterm is used via `ratatui::crossterm` (no separate crossterm dep / version skew).
- **Replaced the thread-based event handling** with a single-threaded poll loop in
  `bin/sparktop.rs` (`event::poll(timeout)` paced by the tick interval). Deleted
  `event.rs` (mpsc + 2 spawned threads) and `sterm.rs` (hand-rolled terminal
  init/restore — now `ratatui::init()` / `ratatui::restore()`, which also installs a
  panic hook).
- **Input cleanups:** Ctrl-C now quits (previously only `q`/`Esc`, a footgun in raw
  mode); only `KeyEventKind::Press` is handled (no double-fire on release); unrecognized
  keys are silently ignored instead of flashing an "unhandled key" alert.
- **Robustness:** layout height math uses `saturating_sub` (a 0/tiny terminal no longer
  panics with subtract-overflow).
- Fixed the `cargo bench` target (`SystemExt` was removed in sysinfo 0.30).
- Deleted `bin/tuitest.rs` (stale scratch pad; recoverable via git).
- **Toolchain note:** ratatui 0.30's newest transitive deps want rustc 1.88, but the
  local toolchain is 1.86. Rather than bump the global toolchain, `Cargo.toml` declares
  `rust-version = "1.86"` and `.cargo/config.toml` sets the MSRV-aware resolver to
  `fallback`, so Cargo picks 1.86-compatible dep versions automatically.

## Architecture

**Core Data Flow:**
1. The main loop (`bin/sparktop.rs`) drives everything single-threaded:
   - `event::poll(timeout)` blocks for input up to the next tick deadline
   - key presses → `view.handle_key`; terminal resizes are handled implicitly by the
     next `draw`; on each tick deadline → `sprocs.update`

2. `SProcs` (sprocs.rs) maintains system process state:
   - Wraps sysinfo's System and tracks all processes in a HashMap<pid, SProc>
   - On each update: refreshes CPU first (required by sysinfo), then processes
   - Applies EWMA (exponentially weighted moving average) to smooth metrics
   - Tracks "tombstones" for dead processes (keeps them visible briefly before removal)

3. `SProc` (sproc.rs) represents a single process with historical data:
   - Maintains EWMA values AND ring buffers (VecDeque, 60 samples) for cpu_hist, disk_read_hist, disk_write_hist
   - SAMPLE_LIMIT=60 caps history length
   - Dead processes get zero-valued samples until tombstone expires

4. `View` + `ViewState` (view.rs, view_state.rs) handle rendering and interaction:
   - ViewState: holds sort column/direction, displayed columns, keyboard action mode
   - View: sorts processes, builds ratatui widgets; `draw` takes the `DefaultTerminal`
     (the main loop owns the terminal), `handle_key` returns `true` to quit
   - Action modes: Top (normal), SelectSort (choose sort key), ToggleColumn (show/hide columns)
   - Keyboard bindings defined in constants (VIEW_ACTIONS, VIEW_SORT_COLUMNS, VIEW_DISPLAY_COLUMNS)

**Key Design Patterns:**
- EWMA smoothing: controllable via `-e/--ewma-weight` flag (0.5 default = 50% new, 50% old)
- Ring buffer history: tracks last 60 samples for sparkline rendering
- Tombstone pattern: dead processes fade out gracefully over SAMPLE_LIMIT ticks
- Mode-based keyboard input: escape always returns to Top mode, other keys context-dependent
- Column system: all columns defined in VIEW_DISPLAY_COLUMNS with char binding, help text, constraint
- Sort system: uses ordered-float crate to sort f64 metrics, negates for descending

**Rendering:**
- Uses `ratatui` with the crossterm backend
- List sparklines in render.rs (`render_vec_colored` / `_multi`); newest sample is
  rightmost (the cpu-history column shows the most-recent `hist_w` samples).
  Detail-view line charts in detail.rs.
- Dead processes shown in red (Color::Red style)
- Layout: main table (or detail view) + footer with keybindings
- Footer text changes based on current Action mode / whether detail is open

**Testing:**
- Unit tests in render.rs (`float_bar`, `cpu_color`, `human_bytes`, multi-height
  bars), sproc.rs (history tracking, ewma), view_state.rs (sort flip, column
  toggle, render_bytes), view.rs (idle filtering).
- detail.rs renders to ratatui `TestBackend` and asserts on the buffer content
  (more robust than scraping terminal escapes); `detail::tests::preview` is an
  `#[ignore]`d visual aid (`cargo test --lib -- --ignored --nocapture`).
- benches/sysinfo_refresh.rs: benchmarks system refresh performance
