/// SProc: a single process.
use std::collections::VecDeque;
use sysinfo::{Pid, Process};

const SAMPLE_LIMIT: usize = 600;

#[derive(Debug)]
pub struct SProc {
    // TODO: ppid, cmd, memory?
    pub pid: Pid,
    pub name: String,
    pub cpu_ewma: f64,
    pub cpu_hist: VecDeque<f64>,
    pub mem_mb: f64,
    // maybe want total bytes over history, and combined read/write?
    pub disk_read_ewma: f64,
    pub disk_read_hist: VecDeque<u64>,
    pub disk_write_ewma: f64,
    pub disk_write_hist: VecDeque<u64>,
    tombstone: Option<Tombstone>,
}

pub enum DeadStatus {
    StillFreshlyDead, // died recently, leave it be
    ShouldReap,       // died a while ago, should remove from list
}

#[derive(Debug)]
struct Tombstone {
    dead_for_ticks: usize, // how ticks has this process been dead for
}

impl SProc {
    pub fn is_dead(&self) -> bool {
        self.tombstone.is_some()
    }

    pub fn add_sample(&mut self, p: &Process, ewma_weight: f64) {
        let du = p.disk_usage();
        self.add_sample_helper(
            p.cpu_usage().into(),
            p.memory(),
            du.read_bytes,
            du.written_bytes,
            ewma_weight,
        );
    }

    pub fn add_dead_sample(&mut self, ewma_weight: f64) -> DeadStatus {
        self.add_sample_helper(0., 0, 0, 0, ewma_weight);
        // probably an off-by-one or two in here but whatevs
        match &mut self.tombstone {
            None => self.tombstone = Some(Tombstone { dead_for_ticks: 1 }),
            Some(ref mut t) => t.dead_for_ticks += 1,
        }
        // LEARN: as_ref, how does it work?
        if self.tombstone.as_ref().unwrap().dead_for_ticks > SAMPLE_LIMIT {
            DeadStatus::ShouldReap
        } else {
            DeadStatus::StillFreshlyDead
        }
    }

    fn add_sample_helper(
        &mut self,
        cpu: f64,
        mem_kb: u64,
        disk_read_bytes: u64,
        disk_write_bytes: u64,
        ewma_weight: f64,
    ) {
        self.cpu_ewma = ewma(cpu, self.cpu_ewma, ewma_weight);
        self.mem_mb = (mem_kb as f64) / 1024.;
        self.disk_read_ewma = ewma(disk_read_bytes as f64, self.disk_read_ewma, ewma_weight);
        self.disk_write_ewma = ewma(disk_write_bytes as f64, self.disk_write_ewma, ewma_weight);
        push_sample(&mut self.cpu_hist, cpu, SAMPLE_LIMIT);
        push_sample(&mut self.disk_read_hist, disk_read_bytes, SAMPLE_LIMIT);
        push_sample(&mut self.disk_write_hist, disk_write_bytes, SAMPLE_LIMIT);
    }
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
            tombstone: None,
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
