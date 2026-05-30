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
- Sparklines rendered in render.rs (`render_vec_colored`); newest sample is leftmost
- Dead processes shown in red (Color::Red style)
- Layout: main table + optional alert box + footer with keybindings
- Footer text changes based on current Action mode
- Note: `ViewState::alert` / the alert box are currently dormant (nothing populates
  `alert` since the "unhandled key" spam was removed). Kept as an error-surface hook —
  wire it up or delete it if it stays unused.

**Testing:**
- `render.rs` has unit tests for `float_bar` / `cpu_color` (run via `cargo test --lib`)
- benches/sysinfo_refresh.rs: benchmarks system refresh performance
