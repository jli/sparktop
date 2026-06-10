# AGENTS.md

sparktop is a terminal system monitor (Rust + [ratatui](https://ratatui.rs) /
crossterm) that augments `top` by keeping **per-process CPU history**, so you
can see "what made everything slow 30 seconds ago" rather than just the current
snapshot. This file is the working guide: how to build it, how it's put
together, and the non-obvious invariants that are easy to break.

## Build & development

```bash
cargo run                      # run with defaults
cargo run -- -d 0.5 -e 0.3     # custom tick delay (0.5s) and EWMA weight (0.3)
cargo test                     # all tests  (cargo test <name> for one)
cargo bench                    # benches/sysinfo_refresh.rs
cargo fmt && cargo clippy      # format + lint
pre-commit run --all-files     # fmt + cargo-check + clippy + hygiene checks
```

Before committing, let the pre-commit hooks run (or run them manually). MSRV is
pinned to **1.86** (`rust-version` in Cargo.toml + the fallback resolver in
`.cargo/config.toml`); don't reach for newer-toolchain-only APIs. TDD is
expected here: write or adjust the failing test first, then the fix.

## Architecture

The main loop (`bin/sparktop.rs`) is single-threaded:

- `event::poll(timeout)` blocks for input up to the next tick deadline; key
  presses go to `view.handle_key`, resizes are handled implicitly by the next
  draw, and each tick deadline calls `sprocs.update` then `view.tick`.
- **Ticks and draws are different clocks.** Draws also happen on every
  keypress, so anything counted in ticks (flash fade, the keep-alive grace
  window) must advance only in `View::tick`, never in `draw` — otherwise
  holding an arrow key burns the counters down in a fraction of a tick.
- `SProcs` (sprocs.rs) wraps sysinfo's `System`, holds every process in a
  `HashMap<Pid, SProc>`, applies EWMA smoothing, and keeps tombstones for dead
  processes so they fade out before removal.
- `SProc` (sproc.rs) is one process: EWMA scalars plus newest-first ring buffers
  (`SAMPLE_LIMIT = 60`) for cpu / mem / disk history.
- `View` + `ViewState` (view.rs, view_state.rs) sort, filter, and render. `View`
  borrows the terminal (the loop owns it); `handle_key` returns `true` to quit.
  Keybindings live in the `VIEW_ACTIONS` / `VIEW_SORT_COLUMNS` /
  `VIEW_DISPLAY_COLUMNS` tables; each column carries its own char, help, and
  width constraint. Input is mode-based (Top / SelectSort / ToggleColumn);
  Esc always returns to Top.
- Rendering: list sparklines in render.rs, detail-view braille line charts in
  detail.rs. Dead processes are drawn red. Layout is the summary header +
  per-core graphs + main table (or detail view) + a context-sensitive footer.
  Sorting uses ordered-float on the f64 metrics (negated for descending).

The subtleties that the code alone won't tell you are below.

## Invariants & principles

### Time alignment: the right edge is always "now"

**Invariant:** in every history graph, a given column represents the same point
in time as the same column in every other graph, and the right edge is the most
recent sample. Samples taken at the same tick line up vertically across all
rows and across the per-core header graphs.

Consequences you must preserve:

- **Right-align, never left-align.** A sparkline with fewer samples than its
  width must be **left-padded with blanks** so its newest sample still lands on
  the right edge. A brand-new process must not "grow from the left"; if it did,
  its newest sample would sit at column 2 while an established process's newest
  sits at the right edge — same time, different column.
  - List CPU-history column: `render::render_cpu_history` (pads, then renders).
  - Per-core header graphs: `view::core_lines` (pads missing samples with blank
    spans, right-aligned within each cell).
- **Dead processes keep marching left.** When a process dies it still receives a
  zero sample every tick (`SProc::add_dead_sample`), so its old activity scrolls
  leftward and the right edge correctly shows "no activity now" instead of
  freezing in place. Don't short-circuit the dead-process update path, and don't
  render dead histories any differently from live ones (apart from color).

### History buffers are newest-first

All ring buffers (`cpu_hist`, `mem_hist`, `disk_*_hist`, `core_hist`) are
`push_front` + `truncate(LIMIT)`, so **index 0 is the newest sample**. Every
display path reverses for rendering (right edge = newest, per above):

- list column: `render_cpu_history` reverses the most-recent slice;
- detail charts: `detail.rs` does `.iter().rev()` so x flows oldest→now;
- aggregation: `sum_hist` sums element-wise *aligned at the front* (newest), not
  by absolute age — so summing histories of different lengths stays correct;
- summary: `SProcs::summary` reverses `core_hist` to hand the view oldest→newest.

If you add a new history field, follow this convention or the graphs read
backwards.

### Layout must not reflow as history fills

The *grid* is derived from terminal width and core count, never from how many
samples have accumulated, so it doesn't jitter while buffers populate over the
first minutes of runtime. The per-core graphs (`view::core_lines`) expand to
fill the row width, but the cores-per-row split stays stable; the list CPU
column pads to `hist_w`. Both blank-pad rather than reflow. `core_hist` keeps a
generous `CORE_HIST_LEN` (256) precisely so the wide graphs have data to fill.
`core_lines_grid_is_balanced_and_stable` asserts the layout is identical with 1
sample vs. a full buffer — keep it green.

### Two CPU metrics: live display vs. sustained rank

`cpu_ewma` is the **displayed** value (user-tunable weight via `-e`). `cpu_rank`
is a separate slow EWMA (`CPU_RANK_WEIGHT`, ~7-tick half-life) used **only for
sort ordering** when sustained mode is on (the default). It starts at 0 so a
fresh spike ranks low and only climbs with sustained load. Display and sort can
legitimately disagree — that's intentional, and the sparkline makes it legible.
Don't collapse these into one value.

Ordering subtlety: the flat list **freezes order** between re-sorts in *instant*
mode (only re-sorting on sort-key or visible-set change, with a pid tiebreak to
kill HashMap shuffle), but **re-sorts every tick** in sustained mode because the
slow metric is smooth enough not to jitter. Tests that assert CPU ordering use
the `instant_view()` helper to opt out of the sustained default.

### Identity is by Pid, not by row index

Selection (`ViewState.selected`), the flash set, and the keep-alive grace window
are all keyed by `Pid`, so they survive re-sorts and filtering. Never cache a
selected *row index* across a draw.

Pid alone isn't identity *across time*, though: the OS reuses pids.
`SProc.start_time` disambiguates — when a pid reappears with a different start
time, `SProcs::update` replaces the whole record (identity, histories,
tombstone) instead of splicing two different processes together.

### sysinfo usage

- **Refresh CPU immediately before processes** in `SProcs::update` — sysinfo
  won't populate per-process CPU otherwise. Order matters; don't reorder.
- **Memory is bytes** (since sysinfo 0.30). Render via `render::human_bytes`.
  Do not divide by 1024 "to get KB" — that was a real bug.
- Per-tick refreshes are deliberately scoped to exactly the fields displayed
  (cpu/memory/disk + user/cmd as `OnlyIfNotSet`). Broadening them regresses CPU
  use; rayon (`multithread`) is disabled in `Cargo.toml` for the same reason.
  Measure steady-state cost via cumulative cputime over a window, not `ps` %CPU
  (which spikes exactly at the tick); `examples/refresh_cputime.rs` under
  `/usr/bin/time -l` does exactly this.
- The sysinfo **0.30 pin is deliberate**: 0.34+ costs ~8% more cputime per
  tick (a per-tick `proc_pidpath` liveness probe for every process whose
  `PROC_PIDTBSDINFO` fails — i.e. every root process when unprivileged).
  Measurements, root cause, and verified migration notes are in
  `docs/sysinfo-upgrade-perf.md` — read it before bumping.

### Functional core, imperative shell

`SProc` / `SProcs` / `View` / `render` are the testable core; `bin/sparktop.rs`
owns the terminal, the single-threaded poll loop, and timing. `View::draw`
borrows the terminal rather than owning it. Keep side effects (I/O, terminal,
clock) in the shell so the core stays unit-testable.

### Tree / aggregate synthetics

- Aggregated rows are synthetic `SProc`s with **no parent**, so they always take
  the flat render path (never the tree path). Their id is the **lowest member
  pid** (stable while that member lives).
- The tree view is rendered **upside-down** (leaves on top, roots at the bottom):
  the DFS output is `reverse()`d and bottom-corner glyphs `╰` are flipped to `╭`.
  Sibling ordering is intentionally the *opposite* of final reading order. If you
  touch `tree_rows`, re-read its comment block before changing the sort key.

## Testing

- Unit tests live next to the code: render.rs (`float_bar`, `cpu_color`,
  `human_bytes`, multi-height bars, cpu-history alignment), sproc.rs (history /
  ewma), view_state.rs (sort flip, column toggle), view.rs (filtering, ordering,
  tree/aggregate, core grid).
- Assert on the rendered `TestBackend` **buffer content** (glyphs), not on
  terminal escape sequences (see detail.rs).
- `#[ignore]`d `*_preview` tests are visual aids:
  `cargo test --lib -- --ignored --nocapture`.
</content>
