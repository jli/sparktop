// Process type.

use std::collections::VecDeque;
use sysinfo::{Process, ProcessExt};

use crate::render;

const SAMPLE_LIMIT: usize = 60;

#[derive(Debug, Default)]
pub struct SProc {
    // TODO: ppid, cmd, memory?
    pub pid: i32,
    pub name: String,
    pub cpu_ewma: f64,
    pub cpu_hist: VecDeque<f64>,
    pub disk_read: VecDeque<u64>,
    pub disk_write: VecDeque<u64>,
}

impl From<&Process> for SProc {
    fn from(p: &Process) -> Self {
        let du = p.disk_usage();
        Self {
            pid: p.pid(),
            name: p.name().into(),
            cpu_ewma: p.cpu_usage().into(),
            // TODO: how does this work..?
            cpu_hist: vec![p.cpu_usage().into()].into(),
            disk_read: vec![du.read_bytes].into(),
            disk_write: vec![du.written_bytes].into(),
        }
    }
}

fn push_sample<T>(deq: &mut VecDeque<T>, x: T, limit: usize) {
    deq.push_front(x);
    deq.truncate(limit);
}

impl SProc {
    pub fn add_sample(&mut self, p: &Process, ewma_weight: f64) {
        let du = p.disk_usage();
        let cpu64: f64 = p.cpu_usage().into();
        self.cpu_ewma = cpu64 * ewma_weight + self.cpu_ewma * (1. - ewma_weight);
        push_sample(&mut self.cpu_hist, p.cpu_usage().into(), SAMPLE_LIMIT);
        push_sample(&mut self.disk_read, du.read_bytes, SAMPLE_LIMIT);
        push_sample(&mut self.disk_write, du.written_bytes, SAMPLE_LIMIT);
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
