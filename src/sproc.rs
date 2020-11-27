// Process type.

use sysinfo::{Process, ProcessExt};

use crate::render;

#[derive(Debug, Default)]
pub struct SProc {
    // TODO: ppid, cmd, memory?
    pub pid: i32,
    pub name: String,
    pub cpu_ewma: f64,
    // TODO: use circular buffer? fixed window?
    pub cpu_hist: Vec<f64>,
    pub disk_read: Vec<u64>,
    pub disk_write: Vec<u64>,
}

impl From<&Process> for SProc {
    fn from(p: &Process) -> Self {
        let du = p.disk_usage();
        Self {
            pid: p.pid(),
            name: p.name().into(),
            cpu_ewma: p.cpu_usage().into(),
            cpu_hist: vec![p.cpu_usage().into()],
            disk_read: vec![du.read_bytes],
            disk_write: vec![du.written_bytes],
        }
    }
}

impl SProc {
    pub fn add_sample(&mut self, p: &Process, ewma_weight: f64) {
        let du = p.disk_usage();
        let cpu64: f64 = p.cpu_usage().into();
        self.cpu_ewma = cpu64 * ewma_weight + self.cpu_ewma * (1. - ewma_weight);
        self.cpu_hist.push(p.cpu_usage().into());
        self.disk_read.push(du.read_bytes);
        self.disk_write.push(du.written_bytes);
    }

    // TODO: maybe remove.
    pub fn _render(&self) -> String {
        format!(
            "{:>6} {:<12} cpu-e: {:4.1} {:>30}",
            self.pid,
            self.name,
            self.cpu_ewma,
            render::render_vec(&self.cpu_hist, 100.)
        )
    }
}
