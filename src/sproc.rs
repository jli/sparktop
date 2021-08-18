/// SProc: a single process.
use std::collections::VecDeque;
use sysinfo::{Process, ProcessExt};

const SAMPLE_LIMIT: usize = 60;

#[derive(Debug, Default)]
pub struct SProc {
    // TODO: ppid, cmd, memory?
    pub pid: i32,
    pub name: String,
    pub cpu_ewma: f64,
    pub cpu_hist: VecDeque<f64>,
    pub mem_mb: f64,
    // maybe want total bytes over history, and combined read/write?
    pub disk_read_ewma: f64,
    pub disk_read_hist: VecDeque<u64>,
    pub disk_write_ewma: f64,
    pub disk_write_hist: VecDeque<u64>,
}

impl From<&Process> for SProc {
    fn from(p: &Process) -> Self {
        let du = p.disk_usage();
        Self {
            pid: p.pid(),
            name: p.name().into(),
            cpu_ewma: p.cpu_usage().into(),
            // TODO: how does the final into() work?
            cpu_hist: vec![p.cpu_usage().into()].into(),
            mem_mb: (p.memory() as f64) / 1024.,
            disk_read_ewma: du.read_bytes as f64, // TODO: how come no into()?
            disk_read_hist: vec![du.read_bytes].into(),
            disk_write_ewma: du.written_bytes as f64,
            disk_write_hist: vec![du.written_bytes].into(),
        }
    }
}

fn ewma(new_val: f64, prev_ewma: f64, ewma_weight: f64) -> f64 {
    new_val * ewma_weight + prev_ewma * (1. - ewma_weight)
}

fn push_sample<T>(deq: &mut VecDeque<T>, x: T, limit: usize) {
    deq.push_front(x);
    deq.truncate(limit);
}

impl SProc {
    pub fn add_sample(&mut self, p: &Process, ewma_weight: f64) {
        let du = p.disk_usage();
        let cpu64: f64 = p.cpu_usage().into();
        let mem_kb = p.memory();
        self.cpu_ewma = ewma(cpu64, self.cpu_ewma, ewma_weight);
        self.mem_mb = (mem_kb as f64) / 1024.;
        self.disk_read_ewma = ewma(du.read_bytes as f64, self.disk_read_ewma, ewma_weight);
        self.disk_write_ewma = ewma(du.written_bytes as f64, self.disk_write_ewma, ewma_weight);
        push_sample(&mut self.cpu_hist, p.cpu_usage().into(), SAMPLE_LIMIT);
        push_sample(&mut self.disk_read_hist, du.read_bytes, SAMPLE_LIMIT);
        push_sample(&mut self.disk_write_hist, du.written_bytes, SAMPLE_LIMIT);
    }
}
