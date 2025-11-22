# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

sparktop is a terminal-based system monitor written in Rust that enhances the traditional `top` utility by tracking historical CPU usage per process. This allows users to see "what caused everything to be slow 30 seconds ago" rather than just current snapshots. The application uses TUI (Terminal User Interface) rendering with crossterm and displays sparklines for CPU history.

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
