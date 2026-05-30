/// ViewState: view model and interactions.
// rendering is done in view.rs
use std::collections::HashSet;

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Constraint;
use sysinfo::Pid;

use crate::sproc::SProc;

/// CPU% below which a process is considered idle and hidden when hide_idle is on.
pub const IDLE_CPU_PCT: f64 = 0.5;

pub struct ViewState {
    pub sort_by: SortColumn,
    pub sort_dir: Dir,
    pub displayed_columns: DisplayedColumns,
    /// Currently-selected process, tracked by pid so it survives re-sorts.
    pub selected: Option<Pid>,
    /// Whether the full-screen process detail view is open.
    pub show_detail: bool,
    /// Hide processes using ~no CPU, to cut clutter. On by default.
    pub hide_idle: bool,
    /// Rows per process in the list, for taller (higher-res) cpu sparklines.
    pub bar_height: u16,
    /// Case-insensitive substring the process name must contain (empty = off).
    pub filter: String,
    /// True while the user is typing into the filter (keys go to the buffer).
    pub filtering: bool,
    pub should_quit: bool,
    action: Action,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            sort_by: SortColumn::default(),
            sort_dir: Dir::default(),
            displayed_columns: DisplayedColumns::default(),
            selected: None,
            show_detail: false,
            hide_idle: true,
            bar_height: 1,
            filter: String::new(),
            filtering: false,
            should_quit: false,
            action: Action::default(),
        }
    }
}

impl ViewState {
    pub fn handle_key(&mut self, key_event: KeyEvent) {
        use Action::*;
        // Unrecognized keys are silently ignored; the footer already advertises
        // what's available in the current mode.
        match (&self.action, key_event.code) {
            (_, KeyCode::Esc) => self.action = Top,
            (&Top, KeyCode::Char('q')) => self.should_quit = true,
            (&Top, KeyCode::Char('i')) => self.hide_idle = !self.hide_idle,
            // cycle bar height 1 -> 2 -> 3 -> 1
            (&Top, KeyCode::Char('b')) => self.bar_height = self.bar_height % 3 + 1,
            (&Top, KeyCode::Char(c)) => {
                if let Some(a) = Action::action_from_char(c) {
                    self.action = a;
                }
            }
            (&SelectSort, KeyCode::Char(c)) => {
                if let Some(col) = Action::sort_col_from_char(c) {
                    // re-selecting the active column flips direction, like
                    // clicking a column header twice
                    if self.sort_by == col {
                        self.sort_dir.flip();
                    } else {
                        self.sort_by = col;
                        self.sort_dir = Dir::Desc;
                    }
                    self.action = Top;
                }
            }
            (&ToggleColumn, KeyCode::Char(c)) => {
                if let Some(col) = Action::display_col_from_char(c) {
                    self.displayed_columns.toggle(col);
                    self.action = Top;
                }
            }
            _ => {}
        };
    }

    pub fn is_top(&self) -> bool {
        self.action == Action::Top
    }

    pub fn footer(&self) -> String {
        if self.filtering {
            return format!("/{}\u{2588}   esc:clear  ⏎:apply", self.filter);
        }
        if self.show_detail {
            return String::from("esc back  ↑↓ prev/next process  q quit");
        }
        match &self.action {
            Action::Top => {
                let mut f = format!(
                    "(/)filter  {}  (i)dle (b)ars  ↑↓ select  ⏎ details  q quit",
                    Action::action_help()
                );
                if !self.filter.is_empty() {
                    f = format!("[filter: {}]  {}", self.filter, f);
                }
                f
            }
            Action::SelectSort => format!("{}  (repeat to reverse)", Action::sort_col_help()),
            Action::ToggleColumn => Action::display_col_help(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Default)]
pub enum Action {
    #[default]
    Top,
    SelectSort,
    ToggleColumn,
}

impl Action {
    fn action_from_char(input_c: char) -> Option<Action> {
        VIEW_ACTIONS
            .iter()
            .find_map(|a| (a.key == input_c).then_some(a.action))
    }

    fn action_help() -> String {
        join_help(VIEW_ACTIONS.iter().map(|a| a.help))
    }

    fn sort_col_from_char(input_c: char) -> Option<SortColumn> {
        VIEW_SORT_COLUMNS
            .iter()
            .find_map(|c| (c.key == input_c).then_some(c.column))
    }

    fn sort_col_help() -> String {
        join_help(VIEW_SORT_COLUMNS.iter().map(|c| c.help))
    }

    fn display_col_from_char(input_c: char) -> Option<DisplayColumn> {
        VIEW_DISPLAY_COLUMNS
            .iter()
            .find_map(|c| (c.key == input_c).then_some(c.column))
    }

    fn display_col_help() -> String {
        join_help(VIEW_DISPLAY_COLUMNS.iter().map(|c| c.help))
    }
}

// note: can use Iterator::intersperse when it's stable
fn join_help<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts.collect::<Vec<_>>().join("  ")
}

struct ViewAction {
    action: Action,
    key: char,
    help: &'static str,
}

const VIEW_ACTIONS: [ViewAction; 2] = [
    ViewAction {
        action: Action::SelectSort,
        key: 's',
        help: "(s)ort",
    },
    ViewAction {
        action: Action::ToggleColumn,
        key: 'c',
        help: "(c)olumns",
    },
];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayColumn {
    Pid,
    ProcessName,
    DiskRead,
    DiskWrite,
    Mem,
    Cpu,
    CpuHistory,
}

pub struct ViewDisplayColumn {
    pub column: DisplayColumn,
    pub key: char,
    pub help: &'static str,
    pub header: &'static str,
    pub constraint: Constraint,
}

const VIEW_DISPLAY_COLUMNS: [ViewDisplayColumn; 7] = [
    ViewDisplayColumn {
        column: DisplayColumn::Pid,
        key: 'p',
        help: "(p)id",
        header: "pid",
        constraint: Constraint::Length(6),
    },
    ViewDisplayColumn {
        column: DisplayColumn::ProcessName,
        key: 'n',
        help: "process-(n)ame",
        header: "process",
        constraint: Constraint::Length(24),
    },
    ViewDisplayColumn {
        column: DisplayColumn::DiskRead,
        key: 'r',
        help: "disk-(r)ead",
        header: "dr",
        constraint: Constraint::Length(7),
    },
    ViewDisplayColumn {
        column: DisplayColumn::DiskWrite,
        key: 'w',
        help: "disk-(w)rite",
        header: "dw",
        constraint: Constraint::Length(7),
    },
    ViewDisplayColumn {
        column: DisplayColumn::Mem,
        key: 'm',
        help: "(m)em",
        header: "mem",
        constraint: Constraint::Length(7),
    },
    ViewDisplayColumn {
        column: DisplayColumn::Cpu,
        key: 'c',
        help: "(c)pu",
        header: "cpu",
        constraint: Constraint::Length(4),
    },
    ViewDisplayColumn {
        column: DisplayColumn::CpuHistory,
        key: 'h',
        help: "cpu-(h)istory",
        header: "cpu history",
        constraint: Constraint::Percentage(100),
    },
];

/// The set of columns currently shown. Iteration order always follows
/// VIEW_DISPLAY_COLUMNS, so column order is stable regardless of toggles.
#[derive(Clone)]
pub struct DisplayedColumns(HashSet<DisplayColumn>);

impl Default for DisplayedColumns {
    fn default() -> Self {
        // everything visible by default
        Self(VIEW_DISPLAY_COLUMNS.iter().map(|c| c.column).collect())
    }
}

impl DisplayedColumns {
    fn toggle(&mut self, col: DisplayColumn) {
        if !self.0.remove(&col) {
            self.0.insert(col);
        }
    }

    pub fn shown(&self) -> Vec<&'static ViewDisplayColumn> {
        VIEW_DISPLAY_COLUMNS
            .iter()
            .filter(|c| self.0.contains(&c.column))
            .collect()
    }

    pub fn header(&self, sort_by: &SortColumn) -> Vec<String> {
        self.shown()
            .iter()
            .map(|c| {
                if sort_by == &c.column {
                    format!("*{}*", c.header)
                } else {
                    String::from(c.header)
                }
            })
            .collect()
    }
}

// hide low values
pub fn render_metric(m: f64) -> String {
    if m < 0.05 {
        String::from("_")
    } else {
        format!("{:.1}", m)
    }
}

// hide near-zero, otherwise human-readable byte counts (e.g. "1.5KB")
pub fn render_bytes(b: f64) -> String {
    if b < 1.0 {
        String::from("_")
    } else {
        crate::render::human_bytes(b)
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum SortColumn {
    Pid,
    #[default]
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
    }
}

impl SortColumn {
    pub fn from_sproc(self, sp: &SProc) -> f64 {
        match self {
            SortColumn::Pid => sp.pid.as_u32() as f64,
            SortColumn::Cpu => sp.cpu_ewma,
            SortColumn::Mem => sp.mem_bytes,
            SortColumn::DiskRead => sp.disk_read_ewma,
            SortColumn::DiskWrite => sp.disk_write_ewma,
            SortColumn::DiskTotal => sp.disk_read_ewma + sp.disk_write_ewma,
        }
    }
}

struct ViewSortColumn {
    column: SortColumn,
    key: char,
    help: &'static str,
}

const VIEW_SORT_COLUMNS: [ViewSortColumn; 6] = [
    ViewSortColumn {
        column: SortColumn::Pid,
        key: 'p',
        help: "(p)id",
    },
    ViewSortColumn {
        column: SortColumn::DiskRead,
        key: 'r',
        help: "disk-(r)ead",
    },
    ViewSortColumn {
        column: SortColumn::DiskWrite,
        key: 'w',
        help: "disk-(w)rite",
    },
    ViewSortColumn {
        column: SortColumn::DiskTotal,
        key: 'd',
        help: "(d)isk-total",
    },
    ViewSortColumn {
        column: SortColumn::Mem,
        key: 'm',
        help: "(m)em",
    },
    ViewSortColumn {
        column: SortColumn::Cpu,
        key: 'c',
        help: "(c)pu",
    },
];

#[derive(Default)]
pub enum Dir {
    Asc,
    #[default]
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn reselecting_sort_column_flips_direction() {
        let mut vs = ViewState::default();
        assert!(matches!(vs.sort_dir, Dir::Desc)); // default

        // switch to a new column: stays Desc
        vs.handle_key(key('s'));
        vs.handle_key(key('m'));
        assert!(matches!(vs.sort_by, SortColumn::Mem));
        assert!(matches!(vs.sort_dir, Dir::Desc));

        // re-select the same column: flips to Asc
        vs.handle_key(key('s'));
        vs.handle_key(key('m'));
        assert!(matches!(vs.sort_dir, Dir::Asc));
    }

    #[test]
    fn toggling_a_column_removes_then_restores_it() {
        let mut vs = ViewState::default();
        let before = vs.displayed_columns.shown().len();
        vs.handle_key(key('c')); // column mode
        vs.handle_key(key('p')); // toggle pid off
        assert_eq!(vs.displayed_columns.shown().len(), before - 1);
        vs.handle_key(key('c'));
        vs.handle_key(key('p')); // back on
        assert_eq!(vs.displayed_columns.shown().len(), before);
    }

    #[test]
    fn render_bytes_hides_zero_and_scales() {
        assert_eq!(render_bytes(0.0), "_");
        assert_eq!(render_bytes(2048.0), "2.0KB");
    }
}
