// benchmark sysinfo methods for refreshing processor info.
//
// compares refresh_all and refresh_processes. refreshing processes (and cpu) is
// just barely faster than refresh_all; refresh_minimal (what sparktop actually
// does per tick) is marginally cheaper still.
//
// NOTE: sysinfo's `multithread` (rayon) feature is *disabled* (see Cargo.toml).
// With rayon, a single refresh has lower wall time (~2.6 ms) but spins up 8
// worker threads that wake and spin-wait every tick; at 1 Hz that thread-pool
// overhead dominated total CPU. Serial refresh has higher per-call wall time
// (~6.8 ms, fine once a second) but ~60% lower *total* CPU and a single thread.
//
// serial wall times (10 iters), ~500 procs:
// refresh_all             time:   [76.2 ms 77.1 ms 78.2 ms]
// refresh_cpu + refresh_processes
//                         time:   [66.7 ms 67.2 ms 67.7 ms]
// refresh_minimal         time:   [67.0 ms 67.6 ms 68.4 ms]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sysinfo::{CpuRefreshKind, ProcessRefreshKind, System, UpdateKind};

fn refresh_all(n: u64) {
    let mut sys = System::new_all();
    for _ in 0..n {
        sys.refresh_all();
    }
}

fn refresh_processes(n: u64) {
    let mut sys = System::new_all();
    for _ in 0..n {
        sys.refresh_processes();
    }
}

// refresh_processes by itself loses CPU info, seems like refresh_cpu is needed
fn refresh_processes_and_cpu(n: u64) {
    let mut sys = System::new_all();
    for _ in 0..n {
        sys.refresh_cpu();
        sys.refresh_processes();
    }
}

// What sparktop actually does per tick: cpu *usage* only (no per-core
// frequency) and only the per-process fields we display.
fn refresh_minimal(n: u64) {
    let mut sys = System::new_all();
    let cpu = CpuRefreshKind::new().with_cpu_usage();
    let procs = ProcessRefreshKind::new()
        .with_cpu()
        .with_memory()
        .with_disk_usage()
        .with_user(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet);
    for _ in 0..n {
        sys.refresh_cpu_specifics(cpu);
        sys.refresh_memory();
        sys.refresh_processes_specifics(procs);
    }
}

fn crit_bench(c: &mut Criterion) {
    c.bench_function("refresh_all", |b| b.iter(|| refresh_all(black_box(10))));
    c.bench_function("refresh_processes", |b| {
        b.iter(|| refresh_processes(black_box(10)))
    });
    c.bench_function("refresh_cpu + refresh_processes", |b| {
        b.iter(|| refresh_processes_and_cpu(black_box(10)))
    });
    c.bench_function("refresh_minimal (sparktop per-tick)", |b| {
        b.iter(|| refresh_minimal(black_box(10)))
    });
}

criterion_group!(benches, crit_bench);
criterion_main!(benches);
