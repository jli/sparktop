# sysinfo upgrade: known per-tick CPU regression

Status (2026-06-09): sparktop is pinned to **sysinfo 0.30** and we have chosen
to stay there for now. Newer sysinfo costs measurably more CPU per tick — the
metric this project optimizes — and the upgrade's benefits don't yet outweigh
that. This doc records the measurements so the next person doesn't have to
redo them (or, worse, upgrade without knowing).

## The numbers

Self-cputime (user+sys) of 300 minimal refreshes (`examples/refresh_cputime.rs`),
quiet machine, ~660 processes, Apple Silicon, macOS 15, Rust 1.96:

| sysinfo | cputime / 300 refreshes | per tick | vs 0.30 |
| ------- | ----------------------- | -------- | ------- |
| 0.30.13 | 1.65–1.74 s             | ~5.7 ms  | —       |
| 0.36.1  | 1.80–1.93 s             | ~6.2 ms  | **+9%** |
| 0.39.3  | 1.77–1.90 s             | ~6.2 ms  | **+8%** |

The ordering held in every interleaved round, on both a loaded and a quiet
machine. The extra cost is almost entirely **sys time** (more kernel work per
refresh) and still present in 0.39.3. Rust 1.86 → 1.96 alone had no measurable
effect — it's sysinfo, not codegen.

In absolute terms: +0.5 ms per 1 Hz tick, ~0.05% of one core. Real but small;
revisit if upstream improves or if an upgrade becomes necessary.

## Root cause (bisected + ablated, 2026-06-09)

A per-version bisect (harness built against 0.30–0.36, interleaved cputime
rounds) shows the jump lands exactly at **0.33 → 0.34**; 0.31–0.33 are at
parity with 0.30. The culprit is upstream commit `62eb2b873` ("Fix macOS issue
where a removed process was still listed", shipped in 0.34):

- macOS can keep dead pids in `proc_listallpids`, so when
  `proc_pidinfo(PROC_PIDTBSDINFO)` fails for a pid, 0.34+ double-checks the
  process is alive via a forced `proc_pidpath` probe — **every refresh**.
- But `PROC_PIDTBSDINFO` also fails (EPERM) for *live* processes you don't own.
  Running unprivileged, that's every root/system process — ~250 of ~700 procs
  on this machine — so the "is it dead?" probe misfires ~250×/tick, adding one
  `proc_pidpath` syscall each (~0.9 µs apiece, all sys time).
- Verified by instrumented counters (the forced branch runs ~248×/tick) and by
  ablation: stubbing out just that branch in a vendored 0.34.2 returns cputime
  to exact 0.33 parity (1.69 vs 1.70 vs stock-0.34's 1.90, in every
  interleaved round). The branch is unchanged through 0.39.3.

Implications: the regression scales with the number of *other users'*
processes, so it would mostly vanish running as root (sparktop doesn't), and
a future upstream fix is plausible — the probe could use `kill(pid, 0)`
(cheaper liveness check) or cache "EPERM but alive" pids instead of re-probing
every tick.

## How to measure (and how not to)

Wall-clock benchmarks (`cargo bench`, criterion) of refresh cost are **too
noisy to compare sysinfo versions** unless the machine is truly idle — an
early wall-clock run under background load showed 0.39 at parity with 0.30,
which turned out to be load noise masking the gap. Instead:

```bash
cargo build --release --example refresh_cputime
/usr/bin/time -l target/release/examples/refresh_cputime 300
```

Compare **user+sys cputime**, interleave the configurations A/B/A/B across
several rounds, and check the ordering holds per-round rather than trusting
single numbers. (Same principle as AGENTS.md's "measure steady-state cost via
cumulative cputime, not `ps` %CPU".) Refresh cost scales with process count,
so only same-session comparisons are meaningful.

## What an upgrade would buy

- **0.39.0**: soundness fix in user enumeration. Pre-0.39 `Users` used libc's
  `getpwent`/`setpwent`/`endpwent`, which are process-global and not
  thread-safe (UB if two threads enumerate concurrently). sparktop calls
  `Users::new_with_refreshed_list()` once, at startup, on a single thread, so
  practical exposure is ~nil — but it's the right hygiene.
- **0.36.0**: CPU-usage fix (upstream PR #1551 / issue #1528) — refreshes
  spaced near `MINIMUM_CPU_UPDATE_INTERVAL` could report 0% per-process CPU.
  At the default 1 s tick sparktop is far from that boundary; very small `-d`
  values could in principle hit it.
- Active maintenance, newer ratatui/crossterm ecosystem compatibility.

## Migration notes (verified by spike, 2026-06-09)

A full port to 0.39.3 is ~30 lines and all tests pass. Requires **MSRV ≥1.95**
(bump `rust-version` in Cargo.toml and the AGENTS.md note). The mechanical
changes:

- `Cargo.toml`: `sysinfo = { version = "0.39", default-features = false,
  features = ["system", "user"] }` (features were split in 0.31).
- `CpuRefreshKind::new()` / `ProcessRefreshKind::new()` → `::nothing()`.
- `refresh_processes_specifics(kind)` →
  `refresh_processes_specifics(ProcessesToUpdate::All, true, kind)`.
- `sys.global_cpu_info().cpu_usage()` → `sys.global_cpu_usage()`.
- `Process::name()` / `cmd()` return `OsStr`/`OsString` — convert via
  `to_string_lossy()`.
- `status_char` needs a `ProcessStatus::Suspended` arm. **Caution:** on
  sysinfo <0.37 that identifier doesn't exist, so the arm silently becomes a
  catch-all binding (rustc warns "unreachable pattern" — don't ignore it).
- Bench: `refresh_cpu()` → `refresh_cpu_usage()`, `refresh_processes()` →
  `refresh_processes(ProcessesToUpdate::All, true)`.
