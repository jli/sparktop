/// View: rendering the UI, interactions.
use anyhow::Result;
use ordered_float::OrderedFloat as OrdFloat;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Layout};
use tui::style::{Modifier, Style};
use tui::widgets::{Block, Borders, Paragraph, Row, Table};
use tui::Terminal;

use crate::{render, sproc::SProc};

#[derive(Copy, Clone, PartialEq)]
enum Metric {
    Cpu,
    Mem,
    DiskRead,
    DiskWrite,
    DiskTotal,
}

impl Metric {
    fn to_header_str(self, sort_by: Metric) -> String {
        use Metric::*;
        let s = match self {
            Cpu => "cpu",
            Mem => "mem",
            DiskRead => "dr",
            DiskWrite => "dw",
            DiskTotal => "d+",
        };
        if sort_by == self || (sort_by == DiskTotal && (self == DiskRead || self == DiskWrite)) {
            format!("*{}*", s)
        } else {
            String::from(s)
        }
    }
}

enum Dir {
    Asc,
    Desc,
}

impl Dir {
    fn flip(&mut self) {
        use Dir::*;
        match self {
            Asc => *self = Desc,
            Desc => *self = Asc,
        }
    }
}

pub struct View {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    sort_by: Metric,
    sort_dir: Dir,
    alert: Option<String>,
}

fn render_disk_bytes(b: f64) -> String {
    if b < 0.05 {
        String::from("_")
    } else {
        b.to_string()
    }
}

impl View {
    pub fn new() -> Result<Self> {
        let stdout = std::io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let terminal = tui::Terminal::new(backend)?;
        // Needed to process key events as they come.
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self {
            terminal,
            sort_by: Metric::Cpu,
            sort_dir: Dir::Desc,
            alert: None,
        })
    }

    fn sort(&self, sprocs: &mut Vec<&SProc>) {
        sprocs.sort_by_key(|&sp| {
            let val = match self.sort_by {
                Metric::Cpu => sp.cpu_ewma,
                Metric::Mem => sp.mem_mb,
                Metric::DiskRead => sp.disk_read_ewma,
                Metric::DiskWrite => sp.disk_write_ewma,
                Metric::DiskTotal => sp.disk_read_ewma + sp.disk_write_ewma,
            };
            match self.sort_dir {
                Dir::Asc => OrdFloat(val),
                Dir::Desc => OrdFloat(-val),
            }
        });
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        let mut unhandled = false;
        match key.code {
            KeyCode::Char('M') => self.sort_by = Metric::Mem,
            KeyCode::Char('P') => self.sort_by = Metric::Cpu,
            KeyCode::Char('R') => self.sort_by = Metric::DiskRead,
            KeyCode::Char('W') => self.sort_by = Metric::DiskWrite,
            KeyCode::Char('D') => self.sort_by = Metric::DiskTotal,
            KeyCode::Char('I') => self.sort_dir.flip(),
            // TODO: nicer exit method...
            KeyCode::Char('q') => panic!("quitting"),
            KeyCode::Esc => (), // clear alert
            KeyCode::Char('l') => {
                // if l but no ctrl, consider unhandled.
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    unhandled = true;
                } // else (ctrl-l) clear alert
            }
            _ => unhandled = true,
        }

        if unhandled {
            self.alert = Some(format!("unhandled key: {:?}", key));
        } else {
            self.alert = None;
        }
    }

    pub fn draw(&mut self, sprocs: &mut Vec<&SProc>) -> Result<()> {
        self.sort(sprocs);
        // erhm, borrow checker workarounds...
        let alert = self.alert.clone();
        let sort_by = self.sort_by.clone();
        self.terminal.clear()?;
        self.terminal.draw(|f| {
            let main_constraints = if alert.is_some() {
                vec![Constraint::Percentage(5), Constraint::Percentage(95)]
            } else {
                vec![Constraint::Min(1)]
            };
            let mut rects = Layout::default()
                .constraints(main_constraints)
                .split(f.size());

            // Draw main panel.
            let main = rects.pop().unwrap(); // main panel last rect
            let proc_table = ProcTable::new(sprocs, sort_by);
            f.render_widget(proc_table.get_table(), main);

            // Draw alert.
            if let Some(alert) = alert {
                let msg = Paragraph::new(alert).block(Block::default().borders(Borders::ALL));
                f.render_widget(msg, rects[0])
            }
        })?;
        Ok(())
    }
}

struct ProcTable<'a> {
    header: Vec<String>,
    sprocs: &'a Vec<&'a SProc>,
}

// LEARN: oof, what's up with this type signature dude.
type MyTable<'a> =
    Table<'a, core::slice::Iter<'a, String>, std::vec::IntoIter<Row<std::vec::IntoIter<String>>>>;

impl<'a> ProcTable<'a> {
    fn new(sprocs: &'a Vec<&SProc>, sort_by: Metric) -> Self {
        use Metric::*;
        let mut header: Vec<String> = vec!["pid", "process"]
            .iter()
            .map(|&s| String::from(s))
            .collect();
        header.extend(
            [DiskRead, DiskWrite, Mem, Cpu]
                .iter()
                .map(|m| m.to_header_str(sort_by)),
        );
        header.push(String::from("cpu history"));
        Self { header, sprocs }
    }

    fn get_table(&self) -> MyTable {
        // LEARN: the collect is just to get a more manageable type :/
        let rows: Vec<_> = self
            .sprocs
            .iter()
            .map(|sp| {
                let d = vec![
                    sp.pid.to_string(),
                    sp.name.clone(),
                    render_disk_bytes(sp.disk_read_ewma),
                    render_disk_bytes(sp.disk_write_ewma),
                    format!("{:.1}", sp.mem_mb),
                    sp.cpu_ewma.to_string(),
                    render::render_vec(&sp.cpu_hist, 100.),
                ];
                // LEARN: why doesn't .iter() work?
                let it = d.into_iter();
                Row::Data(it)
            })
            .collect();
        let tab = Table::new(self.header.iter(), rows.into_iter())
            .header_gap(0)
            .header_style(Style::default().add_modifier(Modifier::UNDERLINED))
            .widths(&[
                Constraint::Length(6),
                Constraint::Length(24),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(4),
                Constraint::Min(10),
            ]);
        tab
    }
}
