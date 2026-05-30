/// SProc: a single process.
use std::collections::VecDeque;
use sysinfo::{Pid, Process, ProcessStatus};

const SAMPLE_LIMIT: usize = 60;

#[derive(Debug)]
pub struct SProc {
    pub pid: Pid,
    pub parent: Option<Pid>,
    pub name: String,
    /// full command line (falls back to name if unavailable)
    pub cmd: String,
    /// owning user name (or uid / "?" if unresolved)
    pub user: String,
    /// single-char process state (R/S/D/Z/T/I/...)
    pub state: char,
    /// thread count (0 if the platform doesn't report it)
    pub threads: usize,
    /// seconds the process has been running
    pub run_secs: u64,
    pub cpu_ewma: f64,
    pub cpu_hist: VecDeque<f64>,
    /// resident memory in bytes (sysinfo reports bytes since 0.30)
    pub mem_bytes: f64,
    pub mem_hist: VecDeque<f64>,
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
        // these can change over a process's life, so refresh each tick
        self.state = status_char(p.status());
        self.threads = p.tasks().map_or(0, |t| t.len());
        self.run_secs = p.run_time();
        self.add_sample_helper(
            p.cpu_usage().into(),
            p.memory(),
            du.read_bytes,
            du.written_bytes,
            ewma_weight,
        );
    }

    pub fn add_dead_sample(&mut self, ewma_weight: f64) -> DeadStatus {
        self.state = 'X';
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
        mem_bytes: u64,
        disk_read_bytes: u64,
        disk_write_bytes: u64,
        ewma_weight: f64,
    ) {
        self.cpu_ewma = ewma(cpu, self.cpu_ewma, ewma_weight);
        self.mem_bytes = mem_bytes as f64;
        self.disk_read_ewma = ewma(disk_read_bytes as f64, self.disk_read_ewma, ewma_weight);
        self.disk_write_ewma = ewma(disk_write_bytes as f64, self.disk_write_ewma, ewma_weight);
        push_sample(&mut self.cpu_hist, cpu, SAMPLE_LIMIT);
        push_sample(&mut self.mem_hist, self.mem_bytes, SAMPLE_LIMIT);
        push_sample(&mut self.disk_read_hist, disk_read_bytes, SAMPLE_LIMIT);
        push_sample(&mut self.disk_write_hist, disk_write_bytes, SAMPLE_LIMIT);
    }
}

impl SProc {
    /// Build from a freshly-sampled process. `user` is resolved by the caller
    /// (which holds the system user table).
    pub fn new(p: &Process, user: String) -> Self {
        let du = p.disk_usage();
        let cmd = p.cmd().join(" ");
        Self {
            pid: p.pid(),
            parent: p.parent(),
            name: p.name().into(),
            cmd: if cmd.is_empty() { p.name().into() } else { cmd },
            user,
            state: status_char(p.status()),
            threads: p.tasks().map_or(0, |t| t.len()),
            run_secs: p.run_time(),
            cpu_ewma: p.cpu_usage().into(),
            cpu_hist: vec![p.cpu_usage().into()].into(),
            mem_bytes: p.memory() as f64,
            mem_hist: vec![p.memory() as f64].into(),
            disk_read_ewma: du.read_bytes as f64,
            disk_read_hist: vec![du.read_bytes].into(),
            disk_write_ewma: du.written_bytes as f64,
            disk_write_hist: vec![du.written_bytes].into(),
            tombstone: None,
        }
    }
}

impl SProc {
    /// Combine several processes that share an aggregation group into one
    /// synthetic row: metrics and histories are summed; the group's id is its
    /// lowest pid (stable as long as that member lives). `name` is the canonical
    /// group name (e.g. "Google Chrome"). `members` must be non-empty.
    pub fn aggregate(name: &str, members: &[&SProc]) -> SProc {
        let rep = members.iter().min_by_key(|m| m.pid.as_u32()).unwrap();
        let base = name;
        let count = members.len();
        let user = if members.iter().all(|m| m.user == members[0].user) {
            members[0].user.clone()
        } else {
            "*".to_string()
        };
        SProc {
            pid: rep.pid,
            parent: None,
            name: format!("{base} ({count})"),
            cmd: format!("{count} × {base}"),
            user,
            state: ' ',
            threads: members.iter().map(|m| m.threads).sum(),
            run_secs: members.iter().map(|m| m.run_secs).max().unwrap_or(0),
            cpu_ewma: members.iter().map(|m| m.cpu_ewma).sum(),
            cpu_hist: sum_hist(members, |m| &m.cpu_hist),
            mem_bytes: members.iter().map(|m| m.mem_bytes).sum(),
            mem_hist: sum_hist(members, |m| &m.mem_hist),
            disk_read_ewma: members.iter().map(|m| m.disk_read_ewma).sum(),
            disk_read_hist: sum_hist(members, |m| &m.disk_read_hist),
            disk_write_ewma: members.iter().map(|m| m.disk_write_ewma).sum(),
            disk_write_hist: sum_hist(members, |m| &m.disk_write_hist),
            tombstone: None,
        }
    }
}

/// Element-wise sum of a history field across members (aligned newest-first).
fn sum_hist<T>(members: &[&SProc], get: impl Fn(&SProc) -> &VecDeque<T>) -> VecDeque<T>
where
    T: Copy + Default + std::ops::Add<Output = T>,
{
    let len = members.iter().map(|m| get(m).len()).max().unwrap_or(0);
    (0..len)
        .map(|i| {
            members.iter().fold(T::default(), |acc, m| {
                get(m).get(i).map_or(acc, |&v| acc + v)
            })
        })
        .collect()
}

fn status_char(s: ProcessStatus) -> char {
    use ProcessStatus::*;
    match s {
        Run => 'R',
        Sleep => 'S',
        Idle => 'I',
        Stop => 'T',
        Zombie => 'Z',
        Tracing => 't',
        Dead => 'X',
        UninterruptibleDiskSleep => 'D',
        Parked => 'P',
        Waking | Wakekill => 'W',
        LockBlocked => 'L',
        Unknown(_) => '?',
    }
}

fn ewma(new_val: f64, prev_ewma: f64, ewma_weight: f64) -> f64 {
    new_val * ewma_weight + prev_ewma * (1. - ewma_weight)
}

fn push_sample<T>(deq: &mut VecDeque<T>, x: T, limit: usize) {
    deq.push_front(x);
    deq.truncate(limit);
}

#[cfg(test)]
impl SProc {
    /// Test-only constructor: a live process with empty histories.
    pub fn blank(pid: u32, name: &str) -> Self {
        Self {
            pid: Pid::from(pid as usize),
            parent: None,
            name: name.to_string(),
            cmd: name.to_string(),
            user: "root".to_string(),
            state: 'R',
            threads: 1,
            run_secs: 0,
            cpu_ewma: 0.0,
            cpu_hist: VecDeque::new(),
            mem_bytes: 0.0,
            mem_hist: VecDeque::new(),
            disk_read_ewma: 0.0,
            disk_read_hist: VecDeque::new(),
            disk_write_ewma: 0.0,
            disk_write_hist: VecDeque::new(),
            tombstone: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histories_track_newest_first_and_cap_at_limit() {
        let mut sp = SProc::blank(1, "t");
        for i in 0..(SAMPLE_LIMIT + 10) {
            sp.add_sample_helper(i as f64, i as u64, i as u64, 0, 1.0);
        }
        assert_eq!(sp.cpu_hist.len(), SAMPLE_LIMIT);
        assert_eq!(sp.mem_hist.len(), SAMPLE_LIMIT);
        assert_eq!(sp.disk_read_hist.len(), SAMPLE_LIMIT);

        // newest sample is at the front
        let newest = (SAMPLE_LIMIT + 9) as f64;
        assert_eq!(sp.cpu_hist.front().copied(), Some(newest));
        assert_eq!(sp.mem_hist.front().copied(), Some(newest));
    }

    #[test]
    fn ewma_blends_old_and_new() {
        assert_eq!(ewma(10.0, 0.0, 1.0), 10.0); // fully new
        assert_eq!(ewma(10.0, 4.0, 0.0), 4.0); // fully old
        assert_eq!(ewma(10.0, 0.0, 0.5), 5.0); // midpoint
    }
}
