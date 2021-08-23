/// ViewState: view model and interactions.
// rendering is done in view.rs

pub struct ViewState {
    pub sort_by: Metric,
    pub sort_dir: Dir,
    pub alert: Option<String>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            sort_by: Metric::Cpu,
            sort_dir: Dir::Desc,
            alert: None,
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum Metric {
    Pid, // not really a "metric"... rename this?
    Cpu,
    Mem,
    DiskRead,
    DiskWrite,
    DiskTotal,
}

pub enum Dir {
    Asc,
    Desc,
}

impl Dir {
    pub fn flip(&mut self) {
        use Dir::*;
        match self {
            Asc => *self = Desc,
            Desc => *self = Asc,
        }
    }
}
