/// ViewState: view model and interactions.
// rendering is done in view.rs
use crossterm::event::{KeyCode, KeyEvent};
use tui::layout::Constraint;

use crate::{render, sproc::SProc};

pub struct ViewState {
    pub sort_by: SortColumn,
    pub sort_dir: Dir,
    pub displayed_columns: DisplayedColumns,
    pub alert: Option<String>,
    pub should_quit: bool,
    action: Action,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            sort_by: SortColumn::Cpu,
            sort_dir: Dir::Desc,
            displayed_columns: DisplayedColumns::default(),
            alert: None,
            should_quit: false,
            action: Action::Top,
        }
    }
}

impl ViewState {
    pub fn handle_key(&mut self, key_event: KeyEvent) {
        let code = key_event.code;
        use Action::*;
        let mut unhandled = false;
        match (&self.action, code) {
            (_, KeyCode::Esc) => self.action = Top,
            (&Top, KeyCode::Char('q')) => self.should_quit = true,
            (&Top, KeyCode::Char(c)) => match Action::action_from_char(c) {
                Some(a) => self.action = a,
                None => unhandled = true,
            },
            (&SelectSort, KeyCode::Char(c)) => match Action::sort_col_from_char(c) {
                Some(col) => {
                    self.sort_by = col;
                    self.action = Top;
                }
                None => unhandled = true,
            },
            (&ToggleColumn, KeyCode::Char(c)) => match Action::display_col_from_char(c) {
                Some(col) => {
                    self.displayed_columns.toggle(col);
                    self.action = Top;
                }
                None => unhandled = true,
            },
            _ => unhandled = true,
        };

        self.alert = if unhandled {
            Some(format!("unhandled key: {:?}", key_event))
        } else {
            None
        };
    }

    pub fn footer(&self) -> String {
        match &self.action {
            Action::Top => Action::action_help(),
            Action::SelectSort => Action::sort_col_help(),
            Action::ToggleColumn => Action::display_col_help(),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum Action {
    Top,
    SelectSort,
    ToggleColumn,
}

impl Action {
    fn action_from_char(input_c: char) -> Option<Action> {
        VIEW_ACTIONS
            .iter()
            .find_map(|ViewAction(a, c, _)| if input_c == *c { Some(*a) } else { None })
    }

    fn action_help() -> String {
        // note: can use iterator::intersperse when it's stable
        let parts: Vec<&str> = VIEW_ACTIONS
            .iter()
            .map(|ViewAction(_, _, help)| help)
            .copied()
            .collect();
        parts.join("  ")
    }

    fn sort_col_from_char(input_c: char) -> Option<SortColumn> {
        VIEW_SORT_COLUMNS
            .iter()
            .find_map(|ViewSortColumn(sc, c, _)| if input_c == *c { Some(*sc) } else { None })
    }

    fn sort_col_help() -> String {
        let parts: Vec<&str> = VIEW_SORT_COLUMNS
            .iter()
            .map(|ViewSortColumn(_, _, help)| help)
            .copied()
            .collect();
        parts.join("  ")
    }

    fn display_col_from_char(input_c: char) -> Option<DisplayColumn> {
        VIEW_DISPLAY_COLUMNS.iter().find_map(
            |ViewDisplayColumn(dc, c, _, _, _)| if input_c == *c { Some(*dc) } else { None },
        )
    }

    fn display_col_help() -> String {
        let parts: Vec<&str> = VIEW_DISPLAY_COLUMNS
            .iter()
            .map(|ViewDisplayColumn(_, _, help, _, _)| help)
            .copied()
            .collect();
        parts.join("  ")
    }
}

struct ViewAction(Action, char, &'static str);

const VIEW_ACTIONS: [ViewAction; 2] = [
    ViewAction(Action::SelectSort, 's', "(s)ort"),
    ViewAction(Action::ToggleColumn, 'c', "(c)olumns"),
];

#[derive(Clone, Copy)]
pub enum DisplayColumn {
    Pid,
    ProcessName,
    DiskRead,
    DiskWrite,
    Mem,
    Cpu,
    CpuHistory,
}

// column, action char, action help, column name
// #[derive(Clone, Copy)]
pub struct ViewDisplayColumn(
    pub DisplayColumn,
    pub char,
    pub &'static str,
    pub &'static str,
    pub Constraint,
);

const VIEW_DISPLAY_COLUMNS: [ViewDisplayColumn; 7] = [
    ViewDisplayColumn(
        DisplayColumn::Pid,
        'p',
        "(p)id",
        "pid",
        Constraint::Length(6),
    ),
    ViewDisplayColumn(
        DisplayColumn::ProcessName,
        'n',
        "process-(n)ame",
        "process",
        Constraint::Length(24),
    ),
    ViewDisplayColumn(
        DisplayColumn::DiskRead,
        'r',
        "disk-(r)ead",
        "dr",
        Constraint::Length(5),
    ),
    ViewDisplayColumn(
        DisplayColumn::DiskWrite,
        'w',
        "diks-(w)rite",
        "dw",
        Constraint::Length(5),
    ),
    ViewDisplayColumn(
        DisplayColumn::Mem,
        'm',
        "(m)em",
        "mem",
        Constraint::Length(5),
    ),
    ViewDisplayColumn(
        DisplayColumn::Cpu,
        'c',
        "(c)pu",
        "cpu",
        Constraint::Length(4),
    ),
    ViewDisplayColumn(
        DisplayColumn::CpuHistory,
        'h',
        "cpu-(h)istory",
        "cpu history",
        Constraint::Percentage(100),
    ),
];

// hmmm, maybe just map from DisplayColumn to bool?
#[derive(Clone, Copy)]
pub struct DisplayedColumns {
    pid: bool,
    process_name: bool,
    disk_read: bool,
    disk_write: bool,
    mem: bool,
    cpu: bool,
    cpu_history: bool,
}

impl Default for DisplayedColumns {
    fn default() -> Self {
        Self {
            pid: true,
            process_name: true,
            disk_read: true,
            disk_write: true,
            mem: true,
            cpu: true,
            cpu_history: true,
        }
    }
}

impl DisplayedColumns {
    fn toggle(&mut self, col: DisplayColumn) {
        use DisplayColumn::*;
        match col {
            Pid => self.pid = !self.pid,
            ProcessName => self.process_name = !self.process_name,
            DiskRead => self.disk_read = !self.disk_read,
            DiskWrite => self.disk_write = !self.disk_write,
            Mem => self.mem = !self.mem,
            Cpu => self.cpu = !self.cpu,
            CpuHistory => self.cpu_history = !self.cpu_history,
        }
    }

    fn should_show(&self, col: &DisplayColumn) -> bool {
        match col {
            DisplayColumn::Pid => self.pid,
            DisplayColumn::ProcessName => self.process_name,
            DisplayColumn::DiskRead => self.disk_read,
            DisplayColumn::DiskWrite => self.disk_write,
            DisplayColumn::Mem => self.mem,
            DisplayColumn::Cpu => self.cpu,
            DisplayColumn::CpuHistory => self.cpu_history,
        }
    }

    pub fn shown(&self) -> Vec<&ViewDisplayColumn> {
        VIEW_DISPLAY_COLUMNS
            .iter()
            .filter(|ViewDisplayColumn(dc, _, _, _, _)| self.should_show(dc))
            .collect()
    }

    pub fn header(&self, sort_by: &SortColumn) -> Vec<String> {
        VIEW_DISPLAY_COLUMNS
            .iter()
            .filter_map(|ViewDisplayColumn(dc, _, _, h, _)| {
                if self.should_show(dc) { Some(h) } else { None }.map(|h| {
                    if sort_by == dc {
                        format!("*{}*", h)
                    } else {
                        String::from(*h)
                    }
                })
            })
            .collect()
    }

    // TODO: gurgh, why can't i just do this in view.rs? oh, maybe because i forgot .iter()..
    pub fn row_data(&self, sp: &SProc) -> Vec<String> {
        use DisplayColumn::*;
        VIEW_DISPLAY_COLUMNS
            .iter()
            .filter_map(|ViewDisplayColumn(dc, _, _, _, _)| {
                if !self.should_show(dc) {
                    return None;
                }
                Some(match dc {
                    Pid => sp.pid.to_string(),
                    ProcessName => sp.name.clone(),
                    DiskRead => render_metric(sp.disk_read_ewma),
                    DiskWrite => render_metric(sp.disk_write_ewma),
                    Mem => render_metric(sp.mem_mb),
                    Cpu => render_metric(sp.cpu_ewma),
                    CpuHistory => render::render_vec(&sp.cpu_hist, 100.),
                })
            })
            .collect()
    }
}

// hide low values
fn render_metric(m: f64) -> String {
    if m < 0.05 {
        String::from("_")
    } else {
        format!("{:.1}", m)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SortColumn {
    Pid,
    Cpu,
    Mem,
    DiskRead,
    DiskWrite,
    DiskTotal,
}

impl PartialEq<DisplayColumn> for SortColumn {
    fn eq(&self, other: &DisplayColumn) -> bool {
        use DisplayColumn as D;
        use SortColumn as S;
        matches!(
            (self, other),
            (S::Pid, D::Pid)
                | (S::Cpu, D::Cpu)
                | (S::Mem, D::Mem)
                | (S::DiskRead, D::DiskRead)
                | (S::DiskWrite, D::DiskWrite)
                | (S::DiskTotal, D::DiskWrite | D::DiskRead)
        )
        // match (self, other) {
        //     (S::Pid, D::Pid)
        //     | (S::Cpu, D::Cpu)
        //     | (S::Mem, D::Mem)
        //     | (S::DiskRead, D::DiskRead)
        //     | (S::DiskWrite, D::DiskWrite)
        //     | (S::DiskTotal, D::DiskWrite | D::DiskRead) => true,
        //     _ => false,
        // }
    }
}

struct ViewSortColumn(SortColumn, char, &'static str);

const VIEW_SORT_COLUMNS: [ViewSortColumn; 6] = [
    ViewSortColumn(SortColumn::Pid, 'p', "(p)id"),
    ViewSortColumn(SortColumn::DiskRead, 'r', "disk-(r)ead"),
    ViewSortColumn(SortColumn::DiskWrite, 'w', "disk-(w)rite"),
    ViewSortColumn(SortColumn::DiskTotal, 'd', "(d)isk-total"),
    ViewSortColumn(SortColumn::Mem, 'm', "(m)em"),
    ViewSortColumn(SortColumn::Cpu, 'c', "(c)pu"),
];

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
