/// View: rendering the UI, interactions.
use anyhow::Result;
use ordered_float::OrderedFloat as OrdFloat;
use ratatui::{
    crossterm::event::KeyEvent,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table},
    DefaultTerminal,
};

use crate::{
    render,
    sproc::SProc,
    view_state::{render_metric, Dir, DisplayColumn, DisplayedColumns, SortColumn, ViewState},
};

#[derive(Default)]
pub struct View {
    state: ViewState,
}

impl View {
    fn sort(&self, sprocs: &mut [&SProc]) {
        sprocs.sort_by_key(|&sp| {
            let val = self.state.sort_by.from_sproc(sp);
            match self.state.sort_dir {
                Dir::Asc => OrdFloat(val),
                Dir::Desc => OrdFloat(-val),
            }
        });
    }

    /// Handle a key press, returning true if the app should quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.state.handle_key(key);
        self.state.should_quit
    }

    pub fn draw(&mut self, terminal: &mut DefaultTerminal, sprocs: &mut [&SProc]) -> Result<()> {
        self.sort(sprocs);
        // erhm, borrow checker workarounds...
        let sort_by = self.state.sort_by;
        let display_columns = self.state.displayed_columns.clone();
        let footer = self.state.footer();
        terminal.draw(|f| {
            let full = f.area();
            let rects = Layout::default()
                .constraints([
                    Constraint::Length(full.height.saturating_sub(1)),
                    Constraint::Length(1),
                ])
                .split(full);

            // Draw process list.
            // need to create constraints here bc the table doesn't take
            // ownership and would be dropping it at the end.
            let constraints: Vec<Constraint> = display_columns
                .shown()
                .iter()
                .map(|c| c.constraint)
                .collect();

            let proc_table = ProcTable::build(sprocs, sort_by, &display_columns, &constraints);
            f.render_widget(proc_table, rects[0]);

            // Draw help footer.
            f.render_widget(
                Paragraph::new(Span::styled(
                    footer,
                    Style::default().add_modifier(Modifier::UNDERLINED),
                ))
                .alignment(Alignment::Right),
                rects[1],
            );
        })?;
        Ok(())
    }
}

struct ProcTable();

impl ProcTable {
    fn build<'a>(
        sprocs: &'a [&SProc],
        sort_by: SortColumn,
        display_columns: &DisplayedColumns,
        constraints: &'a [Constraint],
    ) -> Table<'a> {
        use DisplayColumn::*;

        let header = display_columns.header(&sort_by);
        let vdcols = display_columns.shown();

        let rows = sprocs.iter().map(|sp| {
            let mut liveness_style = Style::default();
            if sp.is_dead() {
                liveness_style = liveness_style.fg(Color::Red);
            }
            let values = vdcols.iter().map(|c| match c.column {
                Pid => Line::from(Span::styled(sp.pid.to_string(), liveness_style)),
                ProcessName => Line::from(Span::styled(sp.name.clone(), liveness_style)),
                DiskRead => Line::from(render_metric(sp.disk_read_ewma)),
                DiskWrite => Line::from(render_metric(sp.disk_write_ewma)),
                Mem => Line::from(render_metric(sp.mem_mb)),
                Cpu => {
                    let text = render_metric(sp.cpu_ewma);
                    match render::cpu_color(sp.cpu_ewma) {
                        Some(color) => Line::from(Span::styled(text, Style::default().fg(color))),
                        None => Line::from(text),
                    }
                }
                CpuHistory => Line::from(render::render_vec_colored(&sp.cpu_hist, 100.)),
            });
            Row::new(values)
        });

        Table::new(rows, constraints.iter().copied())
            .header(Row::new(header).style(Style::default().add_modifier(Modifier::UNDERLINED)))
    }
}
