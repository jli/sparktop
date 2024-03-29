/// View: rendering the UI, interactions.
use anyhow::Result;
use ordered_float::OrderedFloat as OrdFloat;

use crossterm::event::{KeyCode, KeyEvent};
use tui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph, Row, Table},
};

use crate::{
    event::Next,
    sterm::STerm,
    view_state::{Dir, Metric, ViewState},
    {render, sproc::SProc},
};

#[derive(Default)]
pub struct View {
    terminal: STerm,
    state: ViewState,
}

impl View {
    fn sort(&self, sprocs: &mut Vec<&SProc>) {
        sprocs.sort_by_key(|&sp| {
            let val = match self.state.sort_by {
                Metric::Pid => sp.pid as f64,
                Metric::Cpu => sp.cpu_ewma,
                Metric::Mem => sp.mem_mb,
                Metric::DiskRead => sp.disk_read_ewma,
                Metric::DiskWrite => sp.disk_write_ewma,
                Metric::DiskTotal => sp.disk_read_ewma + sp.disk_write_ewma,
            };
            match self.state.sort_dir {
                Dir::Asc => OrdFloat(val),
                Dir::Desc => OrdFloat(-val),
            }
        });
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Next {
        let mut next = Next::Continue;
        let mut unhandled = false;
        match key.code {
            KeyCode::Char('N') => self.state.sort_by = Metric::Pid,
            KeyCode::Char('M') => self.state.sort_by = Metric::Mem,
            KeyCode::Char('P') => self.state.sort_by = Metric::Cpu,
            KeyCode::Char('R') => self.state.sort_by = Metric::DiskRead,
            KeyCode::Char('W') => self.state.sort_by = Metric::DiskWrite,
            KeyCode::Char('D') => self.state.sort_by = Metric::DiskTotal,
            KeyCode::Char('I') => self.state.sort_dir.flip(),
            KeyCode::Char('q') => next = Next::Quit,
            KeyCode::Esc => (), // clear alert
            _ => unhandled = true,
        }

        if unhandled {
            self.state.alert = Some(format!("unhandled key: {:?}", key));
        } else {
            self.state.alert = None;
        }

        next
    }

    pub fn draw(&mut self, sprocs: &mut Vec<&SProc>) -> Result<()> {
        self.sort(sprocs);
        // erhm, borrow checker workarounds...
        let alert = self.state.alert.clone();
        let sort_by = self.state.sort_by;
        self.terminal.draw(|f| {
            let main_constraints = if alert.is_some() {
                vec![Constraint::Min(1), Constraint::Min(3)]
            } else {
                vec![Constraint::Percentage(100)]
            };
            let rects = Layout::default()
                .constraints(main_constraints)
                .split(f.size());

            // Draw main panel.
            let main = rects[0];
            let proc_table = ProcTable::new(sprocs, sort_by);
            f.render_widget(proc_table.get_table(), main);

            // Draw alert.
            if let Some(alert) = alert {
                let msg = Paragraph::new(alert).block(Block::default().borders(Borders::ALL));
                f.render_widget(msg, rects[1])
            }
        })?;
        Ok(())
    }
}

struct ProcTable<'a> {
    header: Vec<String>,
    sprocs: &'a [&'a SProc],
}

impl<'a> ProcTable<'a> {
    fn new(sprocs: &'a [&SProc], sort_by: Metric) -> Self {
        use Metric::*;
        let mut header = vec![String::from("pid"), String::from("process")];
        header.extend(
            [DiskRead, DiskWrite, Mem, Cpu]
                .iter()
                .map(|m| m.to_header_str(sort_by)),
        );
        header.push(String::from("cpu history"));
        Self { header, sprocs }
    }

    fn get_table(&self) -> impl tui::widgets::Widget + '_ {
        let rows = self.sprocs.iter().map(|sp| {
            let mut liveness_style = Style::default();
            if sp.is_dead() {
                liveness_style = liveness_style.fg(tui::style::Color::Red);
            }
            Row::new(
                vec![
                    Span::styled(sp.pid.to_string(), liveness_style),
                    Span::styled(sp.name.clone(), liveness_style),
                    Span::from(render_metric(sp.disk_read_ewma)),
                    Span::from(render_metric(sp.disk_write_ewma)),
                    Span::from(render_metric(sp.mem_mb)),
                    Span::from(render_metric(sp.cpu_ewma)),
                    Span::from(render::render_vec(&sp.cpu_hist, 100.)),
                ]
                .into_iter(),
            )
        });
        Table::new(rows)
            .header(
                // TODO: way to avoid making copy?
                Row::new(self.header.to_vec())
                    .style(Style::default().add_modifier(Modifier::UNDERLINED)),
            )
            .widths(&[
                Constraint::Length(6),
                Constraint::Length(24),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(4),
                Constraint::Percentage(100),
            ])
    }
}

impl Metric {
    fn to_header_str(self, sort_by: Metric) -> String {
        use Metric::*;
        let s = match self {
            Pid => "pid",
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

// hide low values
fn render_metric(m: f64) -> String {
    if m < 0.05 {
        String::from("_")
    } else {
        format!("{:.1}", m)
    }
}
