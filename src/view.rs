/// View: rendering the UI, interactions.

use anyhow::Result;
use ordered_float::OrderedFloat as OrdFloat;

use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Layout};
use tui::style::{Modifier, Style};
use tui::widgets::{Row, Table};
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

pub struct View {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    sort_by: SortBy,
    sort_dir: Dir,
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
        Ok(Self {
            terminal,
            sort_by: SortBy::Cpu,
            sort_dir: Dir::Desc,
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

    pub fn draw(&mut self, sprocs: &mut Vec<&SProc>) -> Result<()> {
        self.sort(sprocs);
        self.terminal.clear()?;
        self.terminal.draw(|f| {
            let rects = Layout::default()
                .constraints([Constraint::Percentage(100)].as_ref())
                .split(f.size());
            let header = ["pid", "process", "mem", "d_r", "d_w", "cpu", "cpu history"];
            let rows = sprocs.iter().map(|sp| {
                // TODO: problem w/ cpu hist rendering:
                // - not aligned in time. when starting a new proc, the drawing starts from the left
                // - after accumulating too much data, bar stops updating
                // current workaround: most recent sample first (left). but that might be weird..?
                let d = vec![
                    sp.pid.to_string(),
                    sp.name.clone(),
                    format!("{:.1}", sp.mem_mb),
                    render_disk_bytes(sp.disk_read_ewma),
                    render_disk_bytes(sp.disk_write_ewma),
                    sp.cpu_ewma.to_string(),
                    render::render_vec(&sp.cpu_hist, 100.),
                ];
                // Learn: why doesn't .iter() work?
                Row::Data(d.into_iter())
            });
            // TODO: how to mix length and percentage?
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
            f.render_widget(tab, rects[0]);
        })?;
        Ok(())
    }
}
