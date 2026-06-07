# AGENTS.md

Non-obvious invariants and principles for working in this codebase. These are
the things that aren't visible from a single function and are easy to break
without noticing. (Build/test commands, the changelog, and the architecture
tour live in `CLAUDE.md`.)

## Time alignment: the right edge is always "now"

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
  - List CPU-history column: `render::render_cpu_history` (pads then renders).
  - Per-core header graphs: `view::core_lines` (pads missing samples with blank
    spans up to `CORE_SPARK_LEN`).
- **Dead processes keep marching left.** When a process dies it still receives a
  zero sample every tick (`SProc::add_dead_sample`), so its old activity scrolls
  leftward and the right edge correctly shows "no activity now" instead of
  freezing in place. Don't short-circuit the dead-process update path, and don't
  render dead histories any differently from live ones (apart from color).

## History buffers are newest-first

All ring buffers (`cpu_hist`, `mem_hist`, `disk_*_hist`, `core_hist`) are
`push_front` + `truncate(LIMIT)`, so **index 0 is the newest sample**. Every
display path reverses for rendering (right edge = newest, per above):

- list column: `render_cpu_history` reverses the most-recent slice;
- detail charts: `detail.rs` does `.iter().rev()` so x flows oldest→now;
- aggregation: `sum_hist` sums element-wise *aligned at the front* (newest), not
  by absolute age — so summing histories of different lengths stays correct;
- summary: `SProcs::summary` reverses `core_hist` to hand the view oldest→newest.

If you add a new history field, follow this convention or the graphs will read
backwards.

## Layout must not reflow as history fills

Widths are reserved up front so the grid doesn't jitter while buffers populate
over the first minute of runtime. `CORE_SPARK_LEN` fixes per-core cell width;
the list CPU column pads to `hist_w`. Tests assert layout stability with 1
sample vs. a full buffer (`core_lines_grid_is_balanced_and_stable`) — keep them
green.

## Two CPU metrics: live display vs. sustained rank

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

## Identity is by Pid, not by row index

Selection (`ViewState.selected`), the flash set, and the keep-alive grace window
are all keyed by `Pid`, so they survive re-sorts and filtering. Never cache a
selected *row index* across a draw.

## sysinfo usage

- **Refresh CPU immediately before processes** in `SProcs::update` — sysinfo
  won't populate per-process CPU otherwise. Order matters; don't reorder.
- **Memory is bytes** (since sysinfo 0.30). Render via `render::human_bytes`.
  Do not divide by 1024 "to get KB" — that was a real bug.
- Per-tick refreshes are deliberately scoped to exactly the fields displayed
  (cpu/memory/disk + user/cmd as `OnlyIfNotSet`). Broadening them regresses CPU
  use; rayon (`multithread`) is disabled in `Cargo.toml` for the same reason.
  Measure steady-state cost via cumulative cputime over a window, not `ps` %CPU
  (which spikes exactly at the tick).

## Functional core, imperative shell

`SProc`/`SProcs`/`View`/`render` are the testable core; `bin/sparktop.rs` owns
the terminal, the single-threaded poll loop, and timing. `View::draw` borrows
the terminal rather than owning it. Keep side effects (I/O, terminal, clock) in
the shell so the core stays unit-testable.

## Tree / aggregate synthetics

- Aggregated rows are synthetic `SProc`s with **no parent**, so they always take
  the flat render path (never the tree path). Their id is the **lowest member
  pid** (stable while that member lives).
- The tree view is rendered **upside-down** (leaves on top, roots at the bottom):
  the DFS output is `reverse()`d and bottom-corner glyphs `╰` are flipped to `╭`.
  Sibling ordering is intentionally the *opposite* of final reading order. If you
  touch `tree_rows`, re-read its comment block before changing the sort key.

## Testing conventions

- Assert on the rendered `TestBackend` **buffer content** (glyphs), not on
  terminal escape sequences (see `detail.rs`).
- `#[ignore]`d `*_preview` tests are visual aids:
  `cargo test --lib -- --ignored --nocapture`.
- TDD is expected here: write/adjust the failing test first, then the fix.
- Run `pre-commit run --all-files` (fmt + cargo-check + clippy) before
  committing. MSRV is pinned to **1.86** (`rust-version` + the fallback resolver
  in `.cargo/config.toml`); don't reach for newer-toolchain-only APIs.
</content>
</invoke>
