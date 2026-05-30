/// View: rendering the UI, interactions.
use std::collections::{HashMap, HashSet};

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
    sprocs::SysSummary,
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

    /// Arrange the visible procs as a parent->child tree in DFS order,
    /// returning (proc, depth). Branches are ordered by their *subtree total* of
    /// the active sort metric (so a quiet parent with a busy child still floats
    /// up), while each row still displays the process's own value. A process
    /// whose parent isn't in the set becomes a root.
    fn tree_rows<'a>(&self, procs: &[&'a SProc]) -> Vec<(&'a SProc, u16)> {
        let present: HashSet<Pid> = procs.iter().map(|p| p.pid).collect();
        let mut children: HashMap<Pid, Vec<&'a SProc>> = HashMap::new();
        let mut roots: Vec<&'a SProc> = Vec::new();
        for &p in procs {
            match p.parent {
                Some(parent) if parent != p.pid && present.contains(&parent) => {
                    children.entry(parent).or_default().push(p)
                }
                _ => roots.push(p),
            }
        }

        // rank each node by the sum of the sort metric across its whole subtree
        let own: HashMap<Pid, f64> = procs
            .iter()
            .map(|p| (p.pid, self.state.sort_by.from_sproc(p)))
            .collect();
        let mut totals: HashMap<Pid, f64> = HashMap::new();
        for &p in procs {
            subtree_total(p.pid, &own, &children, &mut totals, &mut HashSet::new());
        }
        let rank = |sp: &SProc| {
            let t = totals.get(&sp.pid).copied().unwrap_or(0.0);
            match self.state.sort_dir {
                Dir::Asc => OrdFloat(t),
                Dir::Desc => OrdFloat(-t),
            }
        };
        roots.sort_by_key(|&sp| rank(sp));
        for kids in children.values_mut() {
            kids.sort_by_key(|&sp| rank(sp));
        }

        let mut out = Vec::with_capacity(procs.len());
        let mut visited = HashSet::new();
        for &root in &roots {
            push_subtree(root, 0, &children, &mut visited, &mut out);
        }
        out
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

    pub fn draw(
        &mut self,
        terminal: &mut DefaultTerminal,
        summary: &SysSummary,
        sprocs: &[&SProc],
    ) -> Result<()> {
        let mut procs = self.visible(sprocs);
        // rows carry a tree depth (0 in flat mode) for name indentation
        let rows: Vec<(&SProc, u16)> = if self.state.tree {
            self.tree_rows(&procs)
        } else {
            self.sort(&mut procs);
            procs.iter().map(|&sp| (sp, 0)).collect()
        };
        self.order = rows.iter().map(|(sp, _)| sp.pid).collect();
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
        let summary_line = summary_line(summary);
        let table_state = &mut self.table_state;
        terminal.draw(|f| {
            let full = f.area();
            // summary header (1) + main area + footer (1)
            let rects = Layout::default()
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(full.height.saturating_sub(2)),
                    Constraint::Length(1),
                ])
                .split(full);
            f.render_widget(Paragraph::new(summary_line), rects[0]);
            let main = rects[1];
            let footer_area = rects[2];

            if show_detail {
                if let Some(sp) = selected_index.map(|i| rows[i].0) {
                    detail::render_detail(f, main, sp, secs_per_sample);
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
                    ProcTable::build(&rows, sort_by, &display_columns, &constraints, bar_height);
                f.render_stateful_widget(proc_table, main, table_state);
            }

            // Draw help footer.
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

/// One-line system summary: cpu / mem / swap / load / uptime / task count,
/// with cpu and mem shaded by load.
fn summary_line(s: &SysSummary) -> Line<'static> {
    let pct = |used: u64, total: u64| {
        if total > 0 {
            used as f64 / total as f64
        } else {
            0.0
        }
    };
    let mut spans = vec![
        Span::raw("cpu "),
        Span::styled(
            format!("{:>3.0}%", s.cpu_pct),
            Style::default().fg(render::heat(s.cpu_pct / 100.0)),
        ),
        Span::raw("  mem "),
        Span::styled(
            format!(
                "{}/{}",
                render::human_bytes(s.mem_used as f64),
                render::human_bytes(s.mem_total as f64)
            ),
            Style::default().fg(render::heat(pct(s.mem_used, s.mem_total))),
        ),
    ];
    if s.swap_total > 0 {
        spans.push(Span::raw("  swap "));
        spans.push(Span::styled(
            render::human_bytes(s.swap_used as f64),
            Style::default().fg(render::heat(pct(s.swap_used, s.swap_total))),
        ));
    }
    spans.push(Span::raw(format!(
        "  load {:.2} {:.2} {:.2}  up {}  {} tasks",
        s.load.0,
        s.load.1,
        s.load.2,
        fmt_uptime(s.uptime),
        s.tasks
    )));
    Line::from(spans)
}

/// Sum of `own[node]` over `pid` and all its descendants, memoized into
/// `totals`. `visiting` guards against parent cycles.
fn subtree_total(
    pid: Pid,
    own: &HashMap<Pid, f64>,
    children: &HashMap<Pid, Vec<&SProc>>,
    totals: &mut HashMap<Pid, f64>,
    visiting: &mut HashSet<Pid>,
) -> f64 {
    if let Some(&t) = totals.get(&pid) {
        return t;
    }
    let own_val = own.get(&pid).copied().unwrap_or(0.0);
    if !visiting.insert(pid) {
        return own_val; // cycle: count this node once, don't recurse
    }
    let mut total = own_val;
    if let Some(kids) = children.get(&pid) {
        for k in kids {
            total += subtree_total(k.pid, own, children, totals, visiting);
        }
    }
    visiting.remove(&pid);
    totals.insert(pid, total);
    total
}

/// DFS helper for `tree_rows`; `visited` guards against parent cycles.
fn push_subtree<'a>(
    p: &'a SProc,
    depth: u16,
    children: &HashMap<Pid, Vec<&'a SProc>>,
    visited: &mut HashSet<Pid>,
    out: &mut Vec<(&'a SProc, u16)>,
) {
    if !visited.insert(p.pid) {
        return;
    }
    out.push((p, depth));
    if let Some(kids) = children.get(&p.pid) {
        for &k in kids {
            push_subtree(k, depth + 1, children, visited, out);
        }
    }
}

/// Indent a process name by its tree depth (depth 0 = unindented root).
fn indent_name(name: &str, depth: u16) -> String {
    if depth == 0 {
        name.to_string()
    } else {
        format!("{}↳ {}", "  ".repeat(depth as usize - 1), name)
    }
}

fn fmt_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d{h}h")
    } else if h > 0 {
        format!("{h}h{m}m")
    } else {
        format!("{m}m")
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
        rows_data: &'a [(&'a SProc, u16)],
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
        let col_max =
            |f: fn(&SProc) -> f64| rows_data.iter().map(|&(sp, _)| f(sp)).fold(0.0, f64::max);
        let max_cpu = col_max(|sp| sp.cpu_ewma);
        let max_mem = col_max(|sp| sp.mem_bytes);
        let max_dr = col_max(|sp| sp.disk_read_ewma);
        let max_dw = col_max(|sp| sp.disk_write_ewma);

        let rows = rows_data.iter().map(move |&(sp, depth)| {
            let mut liveness_style = Style::default();
            if sp.is_dead() {
                liveness_style = liveness_style.fg(Color::Red);
            }
            let values = vdcols.iter().map(|c| match c.column {
                Pid => Cell::from(Span::styled(sp.pid.to_string(), liveness_style)),
                ProcessName => {
                    Cell::from(Span::styled(indent_name(&sp.name, depth), liveness_style))
                }
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
    fn summary_line_includes_key_stats_and_hides_zero_swap() {
        let s = SysSummary {
            cpu_pct: 42.0,
            mem_used: 8_000_000_000,
            mem_total: 16_000_000_000,
            swap_used: 0,
            swap_total: 0,
            load: (1.0, 2.0, 3.0),
            uptime: 90_061, // 1d 1h 1m
            tasks: 123,
        };
        let text: String = summary_line(&s)
            .spans
            .iter()
            .map(|sp| sp.content.to_string())
            .collect();
        assert!(text.contains("42%"));
        assert!(text.contains("load 1.00 2.00 3.00"));
        assert!(text.contains("up 1d1h"));
        assert!(text.contains("123 tasks"));
        assert!(!text.contains("swap")); // hidden when there's no swap
    }

    #[test]
    fn fmt_uptime_formats() {
        assert_eq!(fmt_uptime(90_061), "1d1h");
        assert_eq!(fmt_uptime(3_700), "1h1m");
        assert_eq!(fmt_uptime(120), "2m");
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
    fn tree_rows_nests_children_and_orphans_become_roots() {
        // 1 -> 2 -> 3 ; 4 standalone ; 5's parent (999) is absent -> root
        let p1 = SProc::blank(1, "init");
        let mut p2 = SProc::blank(2, "child");
        p2.parent = Some(Pid::from(1usize));
        let mut p3 = SProc::blank(3, "grandchild");
        p3.parent = Some(Pid::from(2usize));
        let p4 = SProc::blank(4, "other");
        let mut p5 = SProc::blank(5, "orphan");
        p5.parent = Some(Pid::from(999usize));
        let all = vec![&p1, &p2, &p3, &p4, &p5];

        let rows = View::default().tree_rows(&all);
        assert_eq!(rows.len(), 5);

        let depth = |pid: u32| {
            rows.iter()
                .find(|(sp, _)| sp.pid.as_u32() == pid)
                .unwrap()
                .1
        };
        assert_eq!(
            (depth(1), depth(2), depth(3), depth(4), depth(5)),
            (0, 1, 2, 0, 0)
        );

        let pos = |pid: u32| {
            rows.iter()
                .position(|(sp, _)| sp.pid.as_u32() == pid)
                .unwrap()
        };
        assert!(pos(1) < pos(2) && pos(2) < pos(3)); // children follow their parent
    }

    #[test]
    fn tree_sorts_branches_by_subtree_total_not_own_value() {
        // root A is near-idle (cpu 1) but its child pegs a core (cpu 100);
        // root B uses cpu 50. By subtree total A's branch (101) outranks B (50),
        // so A and its child come first despite A's tiny own value.
        let mut a = SProc::blank(1, "A");
        a.cpu_ewma = 1.0;
        let mut child = SProc::blank(2, "child");
        child.cpu_ewma = 100.0;
        child.parent = Some(Pid::from(1usize));
        let mut b = SProc::blank(3, "B");
        b.cpu_ewma = 50.0;
        let all = vec![&a, &child, &b];

        // default sort is Cpu, Desc
        let rows = View::default().tree_rows(&all);
        let pids: Vec<u32> = rows.iter().map(|(sp, _)| sp.pid.as_u32()).collect();
        assert_eq!(pids, vec![1, 2, 3]); // A, child-of-A, then B
    }

    #[test]
    fn indent_name_indents_by_depth() {
        assert_eq!(indent_name("x", 0), "x");
        assert_eq!(indent_name("x", 1), "↳ x");
        assert_eq!(indent_name("x", 2), "  ↳ x");
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
