/// SProcs: a collection of all processes on the system.
use std::collections::{hash_map::Values, HashMap, VecDeque};

use sysinfo::{Pid, System, Users};

use crate::sproc::{DeadStatus, SProc};

/// Samples of recent per-core usage kept for the header sparklines.
const CORE_HIST_LEN: usize = 16;

pub struct SProcs {
    sys: System,
    users: Users,
    sprocs: HashMap<Pid, SProc>,
    /// recent usage per logical core (newest first), for the header.
    core_hist: Vec<VecDeque<f64>>,
}

/// A snapshot of system-wide stats for the summary header.
pub struct SysSummary {
    pub cpu_pct: f64,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub load: (f64, f64, f64),
    pub uptime: u64,
    pub tasks: usize,
    /// recent usage per core, oldest-to-newest, for compact sparklines.
    pub cores: Vec<Vec<f64>>,
}

impl Default for SProcs {
    fn default() -> Self {
        Self {
            sys: System::new_all(),
            users: Users::new_with_refreshed_list(),
            sprocs: HashMap::default(),
            core_hist: Vec::new(),
        }
    }
}

impl SProcs {
    pub fn update(&mut self, ewma_weight: f64) {
        // Not completely sure why, but we need to refresh cpu immediately
        // before processes for refresh_processes to include cpu usage. This
        // isn't totally crazy, modern cpu power save features can scale things
        // and bust readings.
        self.sys.refresh_cpu();
        self.sys.refresh_memory();

        // record per-core usage history for the header sparklines
        let cpus = self.sys.cpus();
        if self.core_hist.len() != cpus.len() {
            self.core_hist = vec![VecDeque::new(); cpus.len()];
        }
        for (hist, cpu) in self.core_hist.iter_mut().zip(cpus) {
            hist.push_front(cpu.cpu_usage() as f64);
            hist.truncate(CORE_HIST_LEN);
        }

        self.sys.refresh_processes();
        let users = &self.users;
        let latest_procs = self.sys.processes();
        for (&pid, proc) in latest_procs {
            log::debug!("handling {} {} {}", pid, proc.name(), proc.cpu_usage());
            self.sprocs
                .entry(pid)
                .and_modify(|sp| sp.add_sample(proc, ewma_weight))
                .or_insert_with(|| {
                    let user = proc
                        .user_id()
                        .and_then(|uid| users.get_user_by_id(uid))
                        .map(|u| u.name().to_string())
                        .unwrap_or_else(|| "?".to_string());
                    SProc::new(proc, user)
                });
        }

        // TODO: do this more concisely.
        // get dead procs
        let mut dead_procs: Vec<(&Pid, &mut SProc)> = self
            .sprocs
            .iter_mut()
            .filter(|(p, _)| !latest_procs.contains_key(p))
            .collect();
        // add a pseudo-sample for them and filter for procs that should be removed
        let procs_to_reap: Vec<Pid> = dead_procs
            .iter_mut()
            .filter_map(|(&pid, proc)| match proc.add_dead_sample(ewma_weight) {
                DeadStatus::ShouldReap => Some(pid),
                DeadStatus::StillFreshlyDead => None,
            })
            .collect();
        for pid in procs_to_reap {
            log::debug!("removing dead pid: {}", pid);
            self.sprocs.remove(&pid);
        }
    }

    pub fn get(&self) -> Values<Pid, SProc> {
        self.sprocs.values()
    }

    pub fn summary(&self) -> SysSummary {
        let la = System::load_average();
        SysSummary {
            cpu_pct: self.sys.global_cpu_info().cpu_usage() as f64,
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            load: (la.one, la.five, la.fifteen),
            uptime: System::uptime(),
            tasks: self.sys.processes().len(),
            // newest-first internally; reverse to oldest->newest for display
            cores: self
                .core_hist
                .iter()
                .map(|h| h.iter().rev().copied().collect())
                .collect(),
        }
    }
}
