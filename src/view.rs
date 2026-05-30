/// View: rendering the UI, interactions.
use anyhow::Result;
use ordered_float::OrderedFloat as OrdFloat;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent},
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table, TableState},
    DefaultTerminal,
};
use sysinfo::Pid;

use crate::{
    detail, render,
    sproc::SProc,
    view_state::{
        render_bytes, render_metric, Dir, DisplayColumn, DisplayedColumns, SortColumn, ViewState,
    },
};

#[derive(Default)]
pub struct View {
    state: ViewState,
    /// pids in current display order, refreshed each draw; used to translate
    /// up/down navigation into a concrete selected pid.
    order: Vec<Pid>,
    table_state: TableState,
    /// seconds between samples, for labelling the detail-view time axis.
    secs_per_sample: f64,
}

impl View {
    pub fn new(secs_per_sample: f64) -> Self {
        Self {
            secs_per_sample,
            ..Default::default()
        }
    }

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
        // The detail view has its own small key map; up/down flip between
        // processes so you can scan their graphs without leaving it.
        if self.state.show_detail {
            match key.code {
                KeyCode::Esc => self.state.show_detail = false,
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Down => self.move_selection(1),
                KeyCode::Char('q') => self.state.should_quit = true,
                _ => {}
            }
            return self.state.should_quit;
        }
        match key.code {
            KeyCode::Up if self.state.is_top() => self.move_selection(-1),
            KeyCode::Down if self.state.is_top() => self.move_selection(1),
            KeyCode::Enter if self.state.is_top() && self.state.selected.is_some() => {
                self.state.show_detail = true;
            }
            _ => self.state.handle_key(key),
        }
        self.state.should_quit
    }

    /// Move the selection by `delta` rows within the current display order,
    /// clamping at the ends. The first move from no selection lands on the top.
    fn move_selection(&mut self, delta: i32) {
        if self.order.is_empty() {
            return;
        }
        let last = self.order.len() as i32 - 1;
        let next = match self.selected_index() {
            None => 0,
            Some(i) => (i as i32 + delta).clamp(0, last),
        };
        self.state.selected = Some(self.order[next as usize]);
    }

    fn selected_index(&self) -> Option<usize> {
        let pid = self.state.selected?;
        self.order.iter().position(|&p| p == pid)
    }

    pub fn draw(&mut self, terminal: &mut DefaultTerminal, sprocs: &mut [&SProc]) -> Result<()> {
        self.sort(sprocs);
        self.order = sprocs.iter().map(|sp| sp.pid).collect();
        // keep the highlighted row in sync with the selected pid (and let
        // TableState scroll to keep it visible)
        let selected_index = self.selected_index();
        self.table_state.select(selected_index);
        // if the selected process is gone, drop back out of the detail view
        if self.state.show_detail && selected_index.is_none() {
            self.state.show_detail = false;
        }

        // erhm, borrow checker workarounds...
        let sort_by = self.state.sort_by;
        let display_columns = self.state.displayed_columns.clone();
        let footer = self.state.footer();
        let show_detail = self.state.show_detail;
        let secs_per_sample = self.secs_per_sample;
        let table_state = &mut self.table_state;
        terminal.draw(|f| {
            let full = f.area();
            let rects = Layout::default()
                .constraints([
                    Constraint::Length(full.height.saturating_sub(1)),
                    Constraint::Length(1),
                ])
                .split(full);

            if show_detail {
                if let Some(sp) = selected_index.map(|i| sprocs[i]) {
                    detail::render_detail(f, rects[0], sp, secs_per_sample);
                }
            } else {
                // Draw process list.
                // need to create constraints here bc the table doesn't take
                // ownership and would be dropping it at the end.
                let constraints: Vec<Constraint> = display_columns
                    .shown()
                    .iter()
                    .map(|c| c.constraint)
                    .collect();

                let proc_table = ProcTable::build(sprocs, sort_by, &display_columns, &constraints);
                f.render_stateful_widget(proc_table, rects[0], table_state);
            }

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
                DiskRead => Line::from(render_bytes(sp.disk_read_ewma)),
                DiskWrite => Line::from(render_bytes(sp.disk_write_ewma)),
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
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    }
}
