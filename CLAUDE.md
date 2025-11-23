# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

sparktop is a terminal-based system monitor written in Rust that enhances the traditional `top` utility by tracking historical CPU usage per process. This allows users to see "what caused everything to be slow 30 seconds ago" rather than just current snapshots. The application uses TUI (Terminal User Interface) rendering with crossterm and displays sparklines for CPU history.

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

**2025-11-23: Dynamic history compression - simplified**
- Extended SAMPLE_LIMIT from 60 to 600 samples (10 minutes of history)
- Implemented tiered compression that adapts to available terminal width:
  - Tier 0 (0-2min): Full resolution (1 bar = 1 second)
  - Tier 1 (2-5min): 4x compression (1 bar = avg of 4 seconds)
  - Tier 2 (5-10min): 15x compression (1 bar = avg of 15 seconds)
- Visual compression markers in CPU history header show actual compression ratios:
  - Full resolution (1:1): no markers shown (spaces)
  - Compressed sections: displays ratio like "4x" or "15x" when space allows
  - Narrow columns: falls back to single characters ('.' for low, 'o' for medium, 'O' for heavy compression)
  - Colors: Cyan (low compression) → Blue (medium) → Dark Gray (heavy)
- Virtual tiering: when all data is < 120s but exceeds window width, applies progressive compression within that range (e.g., 30s history in 10 slots shows recent at full-res, older compressed)
- **Major simplification**: Rewrote compress_history from scratch (171 lines vs 330 lines)
  - Single clear algorithm: tier0 gets full 1:1 when width >= tier0_samples, otherwise progressive compression
  - No complex branching or edge case handling
  - All 23 tests pass
- Stretch detection: Red '!' markers appear if algorithm tries to expand samples
- Added `cargo test --lib` to pre-commit hooks
- Compression gracefully degrades when terminal width is limited
- Each sample rendered as at most 1 bar (never expands data)

## Architecture

**Core Data Flow:**
1. `EventStream` (event.rs) generates events on separate threads:
   - Tick events at configurable intervals (default 1s) for state updates
   - Key events for user input
   - Resize events for terminal changes

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
   - View: sorts processes, builds TUI widgets, manages terminal drawing
   - Action modes: Top (normal), SelectSort (choose sort key), ToggleColumn (show/hide columns)
   - Keyboard bindings defined in constants (VIEW_ACTIONS, VIEW_SORT_COLUMNS, VIEW_DISPLAY_COLUMNS)

5. Main loop (bin/sparktop.rs):
   - Creates SProcs, View, EventStream
   - Each event → update state or handle key → draw → loop
   - Returns Next::Quit to exit

**Key Design Patterns:**
- EWMA smoothing: controllable via `-e/--ewma-weight` flag (0.5 default = 50% new, 50% old)
- Ring buffer history: tracks last 60 samples for sparkline rendering
- Tombstone pattern: dead processes fade out gracefully over SAMPLE_LIMIT ticks
- Mode-based keyboard input: escape always returns to Top mode, other keys context-dependent
- Column system: all columns defined in VIEW_DISPLAY_COLUMNS with char binding, help text, constraint
- Sort system: uses ordered-float crate to sort f64 metrics, negates for descending

**Rendering:**
- Uses `tui` crate with crossterm backend
- Sparklines rendered in render.rs
- Dead processes shown in red (Color::Red style)
- Layout: main table + optional alert box + footer with keybindings
- Footer text changes based on current Action mode

**Testing:**
- bin/tuitest.rs: separate binary for TUI testing
- benches/sysinfo_refresh.rs: benchmarks system refresh performance
