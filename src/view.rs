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

enum SortBy {
    Cpu,
    Mem,
    DiskRead,
    DiskWrite,
    DiskTotal,
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
    sort_by: SortBy,
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
            sort_by: SortBy::Cpu,
            sort_dir: Dir::Desc,
            alert: None,
        })
    }

    fn sort(&self, sprocs: &mut Vec<&SProc>) {
        sprocs.sort_by_key(|&sp| {
            let val = match self.sort_by {
                SortBy::Cpu => sp.cpu_ewma,
                SortBy::Mem => sp.mem_mb,
                SortBy::DiskRead => sp.disk_read_ewma,
                SortBy::DiskWrite => sp.disk_write_ewma,
                SortBy::DiskTotal => sp.disk_read_ewma + sp.disk_write_ewma,
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
            KeyCode::Char('M') => self.sort_by = SortBy::Mem,
            KeyCode::Char('P') => self.sort_by = SortBy::Cpu,
            KeyCode::Char('R') => self.sort_by = SortBy::DiskRead,
            KeyCode::Char('W') => self.sort_by = SortBy::DiskWrite,
            KeyCode::Char('D') => self.sort_by = SortBy::DiskTotal,
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
        let alert = self.alert.clone();
        self.terminal.clear()?;
        self.terminal.draw(|f| {
            let main_constraints = if alert.is_some() {
                vec![Constraint::Percentage(5), Constraint::Percentage(95)]
            } else {
                vec![Constraint::Min(1)]
            };
            let rects = Layout::default()
                .constraints(main_constraints)
                .split(f.size());

            // Draw main panel.
            // TODO: yuck
            let main = rects[if alert.is_some() { 1 } else { 0 }];
            let header = ["pid", "process", "mem", "d_r", "d_w", "cpu", "cpu history"];
            let rows = sprocs.iter().map(|sp| {
                let d = vec![
                    sp.pid.to_string(),
                    sp.name.clone(),
                    format!("{:.1}", sp.mem_mb),
                    render_disk_bytes(sp.disk_read_ewma),
                    render_disk_bytes(sp.disk_write_ewma),
                    sp.cpu_ewma.to_string(),
                    render::render_vec(&sp.cpu_hist, 100.),
                ];
                // LEARN: why doesn't .iter() work?
                Row::Data(d.into_iter())
            });
            let tab = Table::new(header.iter(), rows)
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
            f.render_widget(tab, main);

            // Draw alert.
            if let Some(alert) = alert {
                let extra = rects[0];
                let msg = Paragraph::new(alert).block(Block::default().borders(Borders::ALL));
                f.render_widget(msg, extra)
            }
        })?;
        Ok(())
    }
}
