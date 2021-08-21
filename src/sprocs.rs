/// SProcs: a collection of all processes on the system.
use std::collections::{hash_map::Values, HashMap};

use sysinfo::{ProcessExt, System, SystemExt};

use crate::sproc::SProc;

pub struct SProcs {
    sys: System,
    sprocs: HashMap<i32, SProc>,
}

impl SProcs {
    pub fn new() -> Self {
        Self {
            sys: System::new_all(),
            sprocs: HashMap::default(),
        }
    }

    pub fn update(&mut self, ewma_weight: f64) {
        // Not completely sure why, but we need to refresh cpu immediately
        // before processes for refresh_processes to include cpu usage. This
        // isn't totally crazy, modern cpu power save features can scale things
        // and bust readings.
        self.sys.refresh_cpu();
        self.sys.refresh_processes();
        let latest_procs = self.sys.get_processes();
        for (&pid, proc) in latest_procs {
            log::debug!("handling {} {} {}", pid, proc.name(), proc.cpu_usage());
            self.sprocs
                .entry(pid)
                .and_modify(|sp| sp.add_sample(proc, ewma_weight))
                .or_insert(proc.into());
        }

        // clean up dead processes
        let dead_pids: Vec<i32> = self
            .sprocs
            .keys()
            .filter(|&p| !latest_procs.contains_key(p))
            .map(|&p| p)
            .collect();
        for dead_pid in dead_pids {
            log::debug!("removing dead pid: {}", dead_pid);
            self.sprocs.remove(&dead_pid);
        }
    }

    pub fn get(&self) -> Values<i32, SProc> {
        self.sprocs.values()
    }
}
