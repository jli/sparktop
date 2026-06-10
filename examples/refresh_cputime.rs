// Run N minimal refreshes (what sparktop does per tick) and exit.
// Meant to be run under `/usr/bin/time -l` to measure self cputime,
// which is robust to background load in a way wall-time benches aren't.
// sysinfo 0.30 API variant.
use sysinfo::{CpuRefreshKind, ProcessRefreshKind, System, UpdateKind};

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
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
    println!("procs={}", sys.processes().len());
}
