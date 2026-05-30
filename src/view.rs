/// View: rendering the UI, interactions.
use anyhow::Result;
use ordered_float::OrderedFloat as OrdFloat;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent},
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    DefaultTerminal,
};
use sysinfo::Pid;

use crate::{
    detail, render,
    sproc::SProc,
    view_state::{
        render_bytes, render_metric, Dir, DisplayColumn, DisplayedColumns, SortColumn, ViewState,
        IDLE_CPU_PCT,
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

    /// The processes to actually display. A name filter (if set) takes
    /// precedence and shows every match; otherwise hide_idle drops near-idle
    /// processes (always keeping the selected one visible).
    fn visible<'a>(&self, sprocs: &[&'a SProc]) -> Vec<&'a SProc> {
        if !self.state.filter.is_empty() {
            let needle = self.state.filter.to_lowercase();
            return sprocs
                .iter()
                .copied()
                .filter(|sp| sp.name.to_lowercase().contains(&needle))
                .collect();
        }
        sprocs
            .iter()
            .copied()
            .filter(|sp| {
                !self.state.hide_idle
                    || sp.cpu_ewma >= IDLE_CPU_PCT
                    || self.state.selected == Some(sp.pid)
            })
            .collect()
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
        // Filter input mode: keystrokes edit the filter text; arrows still move
        // the selection through the (live-filtered) list.
        if self.state.filtering {
            match key.code {
                KeyCode::Esc => {
                    self.state.filter.clear();
                    self.state.filtering = false;
                }
                KeyCode::Enter => self.state.filtering = false,
                KeyCode::Backspace => {
                    self.state.filter.pop();
                }
                KeyCode::Char(c) => self.state.filter.push(c),
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Down => self.move_selection(1),
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::Char('/') if self.state.is_top() => self.state.filtering = true,
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

    pub fn draw(&mut self, terminal: &mut DefaultTerminal, sprocs: &[&SProc]) -> Result<()> {
        let mut procs = self.visible(sprocs);
        self.sort(&mut procs);
        self.order = procs.iter().map(|sp| sp.pid).collect();
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
        let bar_height = self.state.bar_height;
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
                if let Some(sp) = selected_index.map(|i| procs[i]) {
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

                let proc_table =
                    ProcTable::build(&procs, sort_by, &display_columns, &constraints, bar_height);
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

/// A numeric cell shaded by where `val` falls in [0, max] (green->red). Dead
/// processes and near-zero values ("_") are left unshaded so they recede.
fn heat_cell(text: String, val: f64, max: f64, sp: &SProc) -> Cell<'static> {
    if sp.is_dead() || max <= 0.0 || text == "_" {
        Cell::from(text)
    } else {
        Cell::from(Span::styled(
            text,
            Style::default().fg(render::heat(val / max)),
        ))
    }
}

struct ProcTable();

impl ProcTable {
    fn build<'a>(
        sprocs: &'a [&SProc],
        sort_by: SortColumn,
        display_columns: &DisplayedColumns,
        constraints: &'a [Constraint],
        bar_height: u16,
    ) -> Table<'a> {
        use DisplayColumn::*;

        let header = display_columns.header(&sort_by);
        let vdcols = display_columns.shown();

        // per-column maxima (over visible procs) so each numeric column can be
        // shaded relative to its own scale -- big values pop in every column
        let col_max = |f: fn(&SProc) -> f64| sprocs.iter().map(|&sp| f(sp)).fold(0.0, f64::max);
        let max_cpu = col_max(|sp| sp.cpu_ewma);
        let max_mem = col_max(|sp| sp.mem_bytes);
        let max_dr = col_max(|sp| sp.disk_read_ewma);
        let max_dw = col_max(|sp| sp.disk_write_ewma);

        let rows = sprocs.iter().map(move |&sp| {
            let mut liveness_style = Style::default();
            if sp.is_dead() {
                liveness_style = liveness_style.fg(Color::Red);
            }
            let values = vdcols.iter().map(|c| match c.column {
                Pid => Cell::from(Span::styled(sp.pid.to_string(), liveness_style)),
                ProcessName => Cell::from(Span::styled(sp.name.clone(), liveness_style)),
                DiskRead => heat_cell(
                    render_bytes(sp.disk_read_ewma),
                    sp.disk_read_ewma,
                    max_dr,
                    sp,
                ),
                DiskWrite => heat_cell(
                    render_bytes(sp.disk_write_ewma),
                    sp.disk_write_ewma,
                    max_dw,
                    sp,
                ),
                Mem => heat_cell(render_bytes(sp.mem_bytes), sp.mem_bytes, max_mem, sp),
                Cpu => heat_cell(render_metric(sp.cpu_ewma), sp.cpu_ewma, max_cpu, sp),
                // taller bars get more vertical resolution; one Line per row
                CpuHistory => {
                    let lines: Vec<Line> =
                        render::render_vec_colored_multi(&sp.cpu_hist, 100., bar_height as usize)
                            .into_iter()
                            .map(Line::from)
                            .collect();
                    Cell::from(Text::from(lines))
                }
            });
            Row::new(values).height(bar_height)
        });

        Table::new(rows, constraints.iter().copied())
            .header(Row::new(header).style(Style::default().add_modifier(Modifier::UNDERLINED)))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc_with_cpu(pid: u32, cpu: f64) -> SProc {
        let mut sp = SProc::blank(pid, "p");
        sp.cpu_ewma = cpu;
        sp
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(
            KeyCode::Char(c),
            ratatui::crossterm::event::KeyModifiers::NONE,
        )
    }
    fn keycode(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, ratatui::crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn filter_matches_name_substring_case_insensitive() {
        let a = SProc::blank(1, "Firefox");
        let b = SProc::blank(2, "bash");
        let c = SProc::blank(3, "firefox-helper");
        let all = vec![&a, &b, &c];
        let mut v = View::default();
        v.state.filter = "fire".into();
        let vis = v.visible(&all);
        assert_eq!(vis.len(), 2);
        assert!(vis.iter().all(|s| s.name.to_lowercase().contains("fire")));
    }

    #[test]
    fn slash_starts_filter_typing_builds_it_esc_clears() {
        let mut v = View::default();
        v.handle_key(key('/'));
        assert!(v.state.filtering);
        v.handle_key(key('f'));
        v.handle_key(key('o'));
        assert_eq!(v.state.filter, "fo");
        v.handle_key(keycode(KeyCode::Enter)); // apply: keep text, leave input
        assert!(!v.state.filtering);
        assert_eq!(v.state.filter, "fo");
        v.handle_key(key('/'));
        v.handle_key(keycode(KeyCode::Esc)); // clear
        assert!(!v.state.filtering);
        assert_eq!(v.state.filter, "");
    }

    #[test]
    fn hide_idle_filters_low_cpu_but_keeps_selected() {
        let busy = proc_with_cpu(1, 50.0);
        let idle = proc_with_cpu(2, 0.0);
        let idle_selected = proc_with_cpu(3, 0.0);
        let all = vec![&busy, &idle, &idle_selected];

        let mut view = View::default(); // hide_idle on by default
        let vis = view.visible(&all);
        assert!(vis.iter().any(|s| s.pid == busy.pid));
        assert!(!vis.iter().any(|s| s.pid == idle.pid));

        // the selected process stays visible even while idle
        view.state.selected = Some(idle_selected.pid);
        assert!(view
            .visible(&all)
            .iter()
            .any(|s| s.pid == idle_selected.pid));

        // toggling hide_idle off shows everything
        view.state.hide_idle = false;
        assert_eq!(view.visible(&all).len(), 3);
    }
}
