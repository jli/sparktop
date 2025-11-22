/// View: rendering the UI, interactions.
use anyhow::Result;
use crossterm::event::KeyEvent;
use ordered_float::OrderedFloat as OrdFloat;
use tui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Row, Table},
};

use crate::{
    event::Next,
    render,
    sproc::SProc,
    sterm::STerm,
    view_state::{
        render_metric, Dir, DisplayColumn, DisplayedColumns, SortColumn, ViewDisplayColumn,
        ViewState,
    },
};

#[derive(Default)]
pub struct View {
    terminal: STerm,
    state: ViewState,
}

impl View {
    fn sort(&self, sprocs: &mut Vec<&SProc>) {
        sprocs.sort_by_key(|&sp| {
            let val = self.state.sort_by.from_sproc(sp);
            match self.state.sort_dir {
                Dir::Asc => OrdFloat(val),
                Dir::Desc => OrdFloat(-val),
            }
        });
    }

    // ViewState + KeyEvent -> Option<Action>
    // ViewState + Action -> ViewState
    pub fn handle_key(&mut self, key: KeyEvent) -> Next {
        self.state.handle_key(key);
        if self.state.should_quit {
            Next::Quit
        } else {
            Next::Continue
        }
    }

    pub fn draw(&mut self, sprocs: &mut Vec<&SProc>) -> Result<()> {
        self.sort(sprocs);
        // erhm, borrow checker workarounds...
        let alert = self.state.alert.clone();
        let sort_by = self.state.sort_by;
        let display_columns = self.state.displayed_columns;
        let footer = self.state.footer();
        self.terminal.draw(|f| {
            let full = f.size();
            let main_constraints = if alert.is_some() {
                vec![
                    Constraint::Length(full.height - 4),
                    Constraint::Length(3),
                    Constraint::Length(1),
                ]
            } else {
                vec![Constraint::Length(full.height - 1), Constraint::Length(1)]
            };
            let rects = Layout::default().constraints(main_constraints).split(full);

            // Draw process list.
            let table_area = rects[0];
            // let proc_table = ProcTable::new(sprocs, sort_by, display_columns);
            // need to create constraints here bc the table doesn't take
            // ownership and would be dropping it at the end.
            let vdcols = display_columns.shown();
            let constraints: Vec<Constraint> = vdcols
                .iter()
                .map(|ViewDisplayColumn(_, _, _, _, constraint)| constraint)
                .copied()
                .collect();
            let proc_table = ProcTable::build(sprocs, sort_by, display_columns, &constraints);
            f.render_widget(proc_table, table_area);

            // Draw alert.
            if let Some(alert) = alert {
                let msg = Paragraph::new(alert).block(Block::default().borders(Borders::ALL));
                f.render_widget(msg, rects[1])
            }

            // Draw help footer.
            let footer_area = rects[rects.len() - 1];
            f.render_widget(
                Paragraph::new(Span::styled(
                    footer,
                    Style::default().add_modifier(Modifier::UNDERLINED),
                ))
                .alignment(Alignment::Right),
                footer_area,
            );
        })?;
        Ok(())
    }
}

// struct ProcTable<'a> {
//     header: Vec<String>,
//     sprocs: &'a [&'a SProc],
// }
// TODO: make this a Widget
struct ProcTable();

fn cpu_color(cpu: f64) -> Option<Color> {
    if cpu >= 400.0 {
        Some(Color::Magenta)
    } else if cpu >= 200.0 {
        Some(Color::LightMagenta)
    } else if cpu >= 100.0 {
        Some(Color::Red)
    } else {
        None
    }
}

impl ProcTable {
    fn build<'a>(
        sprocs: &'a [&SProc],
        sort_by: SortColumn,
        display_columns: DisplayedColumns,
        constraints: &'a [Constraint],
    ) -> impl tui::widgets::Widget + 'a {
        use DisplayColumn::*;
        let header = display_columns.header(&sort_by);
        let vdcols = display_columns.shown();
        let rows = sprocs.iter().map(|sp| {
            let mut liveness_style = Style::default();
            if sp.is_dead() {
                liveness_style = liveness_style.fg(Color::Red);
            }
            let values = vdcols
                .iter()
                .map(|ViewDisplayColumn(c, _, _, _, _)| match c {
                    Pid => Spans::from(Span::styled(sp.pid.to_string(), liveness_style)),
                    ProcessName => Spans::from(Span::styled(sp.name.clone(), liveness_style)),
                    DiskRead => Spans::from(Span::from(render_metric(sp.disk_read_ewma))),
                    DiskWrite => Spans::from(Span::from(render_metric(sp.disk_write_ewma))),
                    Mem => Spans::from(Span::from(render_metric(sp.mem_mb))),
                    Cpu => {
                        let text = render_metric(sp.cpu_ewma);
                        if let Some(color) = cpu_color(sp.cpu_ewma) {
                            Spans::from(Span::styled(text, Style::default().fg(color)))
                        } else {
                            Spans::from(Span::from(text))
                        }
                    }
                    CpuHistory => Spans::from(render::render_vec_colored(&sp.cpu_hist, 100.)),
                });
            Row::new(values)
        });
        Table::new(rows)
            .header(Row::new(header).style(Style::default().add_modifier(Modifier::UNDERLINED)))
            .widths(constraints)
    }
}
