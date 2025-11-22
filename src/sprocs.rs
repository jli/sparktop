/// SProcs: a collection of all processes on the system.
use std::collections::{hash_map::Values, HashMap};

use sysinfo::{Pid, System};

use crate::sproc::{DeadStatus, SProc};

pub struct SProcs {
    sys: System,
    sprocs: HashMap<Pid, SProc>,
}

impl Default for SProcs {
    fn default() -> Self {
        Self {
            sys: System::new_all(),
            sprocs: HashMap::default(),
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
        self.sys.refresh_processes();
        let latest_procs = self.sys.processes();
        for (&pid, proc) in latest_procs {
            log::debug!("handling {} {} {}", pid, proc.name(), proc.cpu_usage());
            self.sprocs
                .entry(pid)
                .and_modify(|sp| sp.add_sample(proc, ewma_weight))
                .or_insert_with(|| proc.into());
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
}
