// benchmark sysinfo methods for refreshing processor info.
//
// compares refresh_all and refresh_processes. seems refreshing processe (and
// cpu) is just barely faster than refresh_all (31.2), tho i think statistically valid?
//
// refresh_all             time:   [30.772 ms 31.037 ms 31.339 ms]
// refresh_cpu + refresh_processes
//                         time:   [29.911 ms 30.100 ms 30.328 ms]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sysinfo::{System, SystemExt};

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

fn crit_bench(c: &mut Criterion) {
    c.bench_function("refresh_all", |b| b.iter(|| refresh_all(black_box(10))));
    c.bench_function("refresh_processes", |b| {
        b.iter(|| refresh_processes(black_box(10)))
    });
    c.bench_function("refresh_cpu + refresh_processes", |b| {
        b.iter(|| refresh_processes_and_cpu(black_box(10)))
    });
}

criterion_group!(benches, crit_bench);
criterion_main!(benches);
