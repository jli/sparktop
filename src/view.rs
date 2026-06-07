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
    /// Frozen flat-list order, kept stable between re-sorts to stop rows from
    /// jumping every tick. Re-sorted only when the visible set or sort changes.
    flat_order: Vec<Pid>,
    flat_pids: HashSet<Pid>,
    last_sort: Option<(SortColumn, Dir, bool)>,
    /// pids shown last draw, to detect ones that just appeared.
    seen: HashSet<Pid>,
    /// ticks remaining to highlight a freshly-appeared row.
    flash: HashMap<Pid, u8>,
    /// grace ticks remaining for a process that was recently active but has
    /// dipped below the idle threshold; keeps it from flickering out of the
    /// list the moment its smoothed CPU drops.
    keep_alive: HashMap<Pid, u8>,
}

/// How many draws a newly-appeared row stays highlighted as it fades out.
const FLASH_TICKS: u8 = 6;

/// Once a process has been active, keep showing it for at least this many ticks
/// after it falls below the idle threshold. Hysteresis that stops the "appears
/// then vanishes next tick" flicker for processes hovering around the threshold.
const KEEP_ALIVE_TICKS: u8 = 6;

impl View {
    pub fn new(secs_per_sample: f64) -> Self {
        Self {
            secs_per_sample,
            ..Default::default()
        }
    }

    /// True when the CPU sort is using the slow sustained `cpu_rank` metric
    /// (the default) rather than the instant value.
    fn ranks_sustained(&self) -> bool {
        self.state.sustained && self.state.sort_by == SortColumn::Cpu
    }

    /// Value a process is *ordered* by. Normally the active sort metric, but in
    /// sustained mode the CPU sort uses the slow `cpu_rank` EWMA so a transient
    /// spike doesn't jump a process to the top (the displayed CPU stays live).
    fn rank_value(&self, sp: &SProc) -> f64 {
        if self.ranks_sustained() {
            sp.cpu_rank
        } else {
            self.state.sort_by.from_sproc(sp)
        }
    }

    fn sort(&self, sprocs: &mut [&SProc]) {
        sprocs.sort_by_key(|&sp| {
            let val = self.rank_value(sp);
            let signed = match self.state.sort_dir {
                Dir::Asc => OrdFloat(val),
                Dir::Desc => OrdFloat(-val),
            };
            // pid is a stable tiebreak so equal values don't shuffle with the
            // process HashMap's (nondeterministic) iteration order
            (signed, sp.pid.as_u32())
        });
    }

    /// Flat-list rows with frozen ordering: only re-sort when the sort
    /// column/direction changes or the set of visible pids changes (e.g. a
    /// process crosses the idle threshold, spawns, or dies). Otherwise keep the
    /// previous order so rows stay put while their values update in place.
    fn flat_rows<'a>(&mut self, procs: &mut Vec<&'a SProc>) -> Vec<(&'a SProc, String)> {
        let pids: HashSet<Pid> = procs.iter().map(|p| p.pid).collect();
        let sort_key = (
            self.state.sort_by,
            self.state.sort_dir,
            self.state.sustained,
        );
        // In sustained mode the sort key (slow cpu_rank) is smooth, so re-sort
        // every tick for gradual easing — the jitter that motivated freezing the
        // order only happens under the noisy instant metric.
        if self.ranks_sustained() || self.last_sort != Some(sort_key) || pids != self.flat_pids {
            self.sort(procs);
            self.flat_order = procs.iter().map(|p| p.pid).collect();
            self.flat_pids = pids;
            self.last_sort = Some(sort_key);
        } else {
            let by_pid: HashMap<Pid, &SProc> = procs.iter().map(|&p| (p.pid, p)).collect();
            *procs = self
                .flat_order
                .iter()
                .filter_map(|pid| by_pid.get(pid).copied())
                .collect();
        }
        procs.iter().map(|&sp| (sp, String::new())).collect()
    }

    /// A process counts as "active" if it's currently at/above the idle
    /// threshold, or still inside its post-activity grace window.
    fn is_active(&self, sp: &SProc) -> bool {
        sp.cpu_ewma >= IDLE_CPU_PCT || self.keep_alive.contains_key(&sp.pid)
    }

    /// Advance the grace window: any process at/above the idle threshold resets
    /// its window to KEEP_ALIVE_TICKS; everything else ages by one and is dropped
    /// once it expires (or the process is gone). Call once per draw, before
    /// `visible`, so a briefly-busy process lingers instead of flickering out.
    fn update_keep_alive(&mut self, sprocs: &[&SProc]) {
        for &sp in sprocs {
            if sp.cpu_ewma >= IDLE_CPU_PCT {
                self.keep_alive.insert(sp.pid, KEEP_ALIVE_TICKS);
            }
        }
        let present: HashSet<Pid> = sprocs.iter().map(|p| p.pid).collect();
        self.keep_alive.retain(|pid, n| {
            *n = n.saturating_sub(1);
            *n > 0 && present.contains(pid)
        });
    }

    /// The processes to actually display.
    /// - a name filter (if set) takes precedence and shows every match;
    /// - tree mode with hide_idle prunes to active branches plus their full
    ///   ancestor chains (so the lineage stays intact); without hide_idle it
    ///   shows the whole tree;
    /// - the flat list applies hide_idle, always keeping the selected one.
    fn visible<'a>(&self, sprocs: &[&'a SProc]) -> Vec<&'a SProc> {
        if !self.state.filter.is_empty() {
            let needle = self.state.filter.to_lowercase();
            return sprocs
                .iter()
                .copied()
                .filter(|sp| sp.name.to_lowercase().contains(&needle))
                .collect();
        }
        if self.state.tree {
            if !self.state.hide_idle {
                return sprocs.to_vec(); // full tree
            }
            return self.active_branches(sprocs);
        }
        sprocs
            .iter()
            .copied()
            .filter(|sp| {
                !self.state.hide_idle || self.is_active(sp) || self.state.selected == Some(sp.pid)
            })
            .collect()
    }

    /// Active (or selected) processes plus every ancestor up to the root, so the
    /// tree keeps each busy process's lineage but drops unrelated idle branches.
    fn active_branches<'a>(&self, sprocs: &[&'a SProc]) -> Vec<&'a SProc> {
        let by_pid: HashMap<Pid, &SProc> = sprocs.iter().map(|&p| (p.pid, p)).collect();
        let mut keep: HashSet<Pid> = HashSet::new();
        for &p in sprocs {
            if !self.is_active(p) && self.state.selected != Some(p.pid) {
                continue;
            }
            // walk up from this active node, stopping once we reach a node (and
            // therefore a chain) we've already kept
            let mut cur = Some(p.pid);
            while let Some(pid) = cur {
                if !keep.insert(pid) {
                    break;
                }
                cur = by_pid.get(&pid).and_then(|sp| sp.parent);
            }
        }
        sprocs
            .iter()
            .copied()
            .filter(|p| keep.contains(&p.pid))
            .collect()
    }

    /// Arrange the visible procs as a parent->child tree in DFS order,
    /// returning (proc, depth). Branches are ordered by their *subtree total* of
    /// the active sort metric (so a quiet parent with a busy child still floats
    /// up), while each row still displays the process's own value. A process
    /// whose parent isn't in the set becomes a root.
    fn tree_rows<'a>(&self, procs: &[&'a SProc]) -> Vec<(&'a SProc, String)> {
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
        let own: HashMap<Pid, f64> = procs.iter().map(|p| (p.pid, self.rank_value(p))).collect();
        let mut totals: HashMap<Pid, f64> = HashMap::new();
        for &p in procs {
            subtree_total(p.pid, &own, &children, &mut totals, &mut HashSet::new());
        }
        // The output is mirror-reversed at the end (leaves on top, roots at the
        // bottom), so order siblings here in the *opposite* of how they should
        // read. The key is (is_internal, by_total):
        //   * is_internal sorts leaf children before subtree children top-down,
        //     so after the flip each parent's leaf children are grouped directly
        //     above it and its subtrees stack higher. This keeps a parent tight
        //     against its own children and stops a lone leaf from being wedged
        //     between another sibling's subtree rows (the "split branch" look).
        //   * by_total then ranks by subtree total, flipped so the busiest
        //     branch ends up toward the top after the reversal.
        let internal: HashSet<Pid> = children.keys().copied().collect();
        let rank = |sp: &SProc| {
            let t = totals.get(&sp.pid).copied().unwrap_or(0.0);
            let by_total = match self.state.sort_dir {
                Dir::Asc => OrdFloat(-t),
                Dir::Desc => OrdFloat(t),
            };
            (internal.contains(&sp.pid), by_total)
        };
        roots.sort_by_key(|&sp| rank(sp));
        for kids in children.values_mut() {
            kids.sort_by_key(|&sp| rank(sp));
        }

        let mut out = Vec::with_capacity(procs.len());
        let mut visited = HashSet::new();
        for &root in &roots {
            push_subtree(root, "", true, true, &children, &mut visited, &mut out);
        }
        // Flip the whole thing upside-down: leaves on top, roots at the bottom.
        // The DFS built connectors for a top-down tree, so flip each bottom
        // corner (╰) to a top corner (╭); ├ and │ are vertically symmetric.
        out.reverse();
        for (_, prefix) in &mut out {
            *prefix = prefix.replace('╰', "╭");
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

    /// Mark rows that just entered the visible set so they can flash. The first
    /// population (startup) is not flashed.
    fn note_new_rows(&mut self, current: &HashSet<Pid>) {
        if !self.seen.is_empty() {
            for &pid in current {
                if !self.seen.contains(&pid) {
                    self.flash.insert(pid, FLASH_TICKS);
                }
            }
        }
        self.seen = current.clone();
    }

    /// Age out the flash highlights by one draw.
    fn fade_flashes(&mut self) {
        self.flash.retain(|_, n| {
            *n = n.saturating_sub(1);
            *n > 0
        });
    }

    pub fn draw(
        &mut self,
        terminal: &mut DefaultTerminal,
        summary: &SysSummary,
        sprocs: &[&SProc],
    ) -> Result<()> {
        // advance the post-activity grace window before deciding what's visible
        self.update_keep_alive(sprocs);
        let visible = self.visible(sprocs);
        // aggregate mode replaces the rows with summed per-name synthetics;
        // they're owned here and borrowed for the rest of the draw.
        let aggregated: Vec<SProc> = if self.state.aggregate {
            aggregate_by_name(&visible)
        } else {
            Vec::new()
        };
        let mut procs: Vec<&SProc> = if self.state.aggregate {
            aggregated.iter().collect()
        } else {
            visible
        };
        let current_pids: HashSet<Pid> = procs.iter().map(|p| p.pid).collect();
        self.note_new_rows(&current_pids);
        // each row carries a tree-indent prefix ("" in flat mode).
        // aggregate rows have no parents, so they always use the flat path.
        let rows: Vec<(&SProc, String)> = if self.state.tree && !self.state.aggregate {
            self.tree_rows(&procs)
        } else {
            self.flat_rows(&mut procs)
        };
        self.order = rows.iter().map(|r| r.0.pid).collect();
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
        let sort_dir = self.state.sort_dir;
        let display_columns = self.state.displayed_columns.clone();
        let footer = self.state.footer();
        let show_detail = self.state.show_detail;
        let bar_height = self.state.bar_height;
        let secs_per_sample = self.secs_per_sample;
        let summary_line = summary_line(summary);
        let cores = &summary.cores;
        let flash = &self.flash;
        let selected = self.state.selected;
        let table_state = &mut self.table_state;
        terminal.draw(|f| {
            let full = f.area();
            // summary header (1) + per-core sparklines + main area + footer (1)
            let core_lines = core_lines(cores, full.width);
            let core_h = core_lines.len() as u16;
            let rects = Layout::default()
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(core_h),
                    Constraint::Length(full.height.saturating_sub(2 + core_h)),
                    Constraint::Length(1),
                ])
                .split(full);
            f.render_widget(Paragraph::new(summary_line), rects[0]);
            f.render_widget(Paragraph::new(core_lines), rects[1]);
            let main = rects[2];
            let footer_area = rects[3];

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

                // width available to the cpu-history sparkline, so we can show
                // exactly the most-recent N samples (newest on the right)
                let fixed: u16 = constraints
                    .iter()
                    .map(|c| if let Constraint::Length(n) = c { *n } else { 0 })
                    .sum();
                let hist_w = main
                    .width
                    .saturating_sub(fixed + constraints.len() as u16)
                    .max(1) as usize;

                let header = display_columns.header(&sort_by, sort_dir);
                let proc_table = ProcTable::build(
                    &rows,
                    header,
                    &display_columns,
                    &constraints,
                    bar_height,
                    hist_w,
                    &Highlights { flash, selected },
                );
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
        self.fade_flashes();
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
        render::fmt_uptime(s.uptime),
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

/// DFS helper for `tree_rows`, building a box-drawing indent prefix per row
/// (e.g. "│  ├─ "). `visited` guards against parent cycles.
fn push_subtree<'a>(
    p: &'a SProc,
    prefix: &str,
    is_last: bool,
    is_root: bool,
    children: &HashMap<Pid, Vec<&'a SProc>>,
    visited: &mut HashSet<Pid>,
    out: &mut Vec<(&'a SProc, String)>,
) {
    if !visited.insert(p.pid) {
        return;
    }
    let connector = if is_root {
        ""
    } else if is_last {
        "╰─ "
    } else {
        "├─ "
    };
    out.push((p, format!("{prefix}{connector}")));

    // children are indented under this node; continue the vertical line unless
    // this was the last child
    let child_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{prefix}   ")
    } else {
        format!("{prefix}│  ")
    };
    if let Some(kids) = children.get(&p.pid) {
        let last = kids.len() - 1;
        for (i, &k) in kids.iter().enumerate() {
            push_subtree(k, &child_prefix, i == last, false, children, visited, out);
        }
    }
}

/// Width (in chars) reserved for each core's sparkline, regardless of how many
/// samples have accumulated, so the grid doesn't reflow as history fills in.
const CORE_SPARK_LEN: usize = 16;
/// Preferred cores per row (so an 8-core machine shows 4 per row, 2 rows);
/// reduced to fit narrow terminals.
const CORE_PER_ROW: usize = 4;

/// Compact per-core usage sparklines for the header, drawn two terminal lines
/// tall for extra vertical resolution: the bottom cell covers 0-50% and the top
/// cell 50-100%, each colored by load. Cells are fixed width (bars grow from the
/// right as history fills) and laid out in a balanced grid that targets
/// CORE_PER_ROW per row but adapts to the terminal width and core count. Each
/// grid row therefore emits two `Line`s (top half, then bottom half).
fn core_lines(cores: &[Vec<f64>], width: u16) -> Vec<Line<'static>> {
    if cores.is_empty() {
        return Vec::new();
    }
    let n = cores.len();
    let label_w = (n - 1).to_string().len();
    let cell_w = label_w + 1 + CORE_SPARK_LEN + 1; // "N " + spark + gap
    let fit = (width as usize / cell_w).max(1);
    // target up to CORE_PER_ROW, capped by what fits and the core count, then
    // balance evenly across the resulting number of rows
    let target = CORE_PER_ROW.min(fit).min(n);
    let rows = n.div_ceil(target);
    let per_row = n.div_ceil(rows);

    // each core renders to a (top, bottom) pair of span rows
    let cells: Vec<(Vec<Span>, Vec<Span>)> = cores
        .iter()
        .enumerate()
        .map(|(i, samples)| {
            // label sits on the bottom (baseline) row; the top row is blank over
            // the label column
            let mut top = vec![Span::raw(" ".repeat(label_w + 1))];
            let mut bottom = vec![Span::styled(
                format!("{i:>label_w$} "),
                Style::default().fg(Color::DarkGray),
            )];
            // right-align the bars in a fixed field: pad missing samples so the
            // layout stays put while the history populates
            let shown = samples.len().min(CORE_SPARK_LEN);
            for _ in 0..CORE_SPARK_LEN - shown {
                top.push(Span::raw(" "));
                bottom.push(Span::raw(" "));
            }
            for &v in samples.iter().take(CORE_SPARK_LEN) {
                let frac = (v / 100.0).clamp(0.0, 1.0);
                let color = render::heat(frac);
                // split 0-100% across two rows: bottom = 0-50%, top = 50-100%
                let bottom_local = (frac * 2.0).clamp(0.0, 1.0);
                let top_local = (frac * 2.0 - 1.0).clamp(0.0, 1.0);
                top.push(Span::styled(
                    render::float_bar(top_local).to_string(),
                    Style::default().fg(color),
                ));
                bottom.push(Span::styled(
                    render::float_bar(bottom_local).to_string(),
                    Style::default().fg(color),
                ));
            }
            top.push(Span::raw(" "));
            bottom.push(Span::raw(" "));
            (top, bottom)
        })
        .collect();

    cells
        .chunks(per_row)
        .flat_map(|chunk| {
            let top: Vec<Span> = chunk.iter().flat_map(|(t, _)| t.iter().cloned()).collect();
            let bottom: Vec<Span> = chunk.iter().flat_map(|(_, b)| b.iter().cloned()).collect();
            vec![Line::from(top), Line::from(bottom)]
        })
        .collect()
}

/// Canonical group name for "aggregate by name": collapses helper/role
/// processes onto their parent app, so e.g. "Google Chrome", "Google Chrome
/// Helper" and "Google Chrome Helper (Renderer)" all fold into "Google Chrome".
fn aggregate_key(name: &str) -> &str {
    let mut key = name;
    // drop a trailing role parenthetical, e.g. " (Renderer)", " (GPU)"
    if key.ends_with(')') {
        if let Some(open) = key.rfind(" (") {
            key = &key[..open];
        }
    }
    // drop a trailing " Helper" (Chrome/Electron-style child processes)
    key.strip_suffix(" Helper").unwrap_or(key)
}

/// Fold processes sharing an aggregation key into one summed synthetic row each.
fn aggregate_by_name(procs: &[&SProc]) -> Vec<SProc> {
    let mut groups: HashMap<&str, Vec<&SProc>> = HashMap::new();
    for &p in procs {
        groups.entry(aggregate_key(&p.name)).or_default().push(p);
    }
    groups
        .into_iter()
        .map(|(name, g)| SProc::aggregate(name, &g))
        .collect()
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

/// Per-row name-cell highlighting: the amber new-row flash and the selection
/// reverse-video both apply to just the process name (not the tree indent).
struct Highlights<'a> {
    flash: &'a HashMap<Pid, u8>,
    selected: Option<Pid>,
}

struct ProcTable();

impl ProcTable {
    fn build<'a>(
        rows_data: &'a [(&'a SProc, String)],
        header: Vec<String>,
        display_columns: &DisplayedColumns,
        constraints: &'a [Constraint],
        bar_height: u16,
        hist_w: usize,
        hl: &Highlights,
    ) -> Table<'a> {
        use DisplayColumn::*;

        let vdcols = display_columns.shown();

        // per-column maxima (over visible procs) so each numeric column can be
        // shaded relative to its own scale -- big values pop in every column
        let col_max = |f: fn(&SProc) -> f64| rows_data.iter().map(|r| f(r.0)).fold(0.0, f64::max);
        let max_cpu = col_max(|sp| sp.cpu_ewma);
        let max_mem = col_max(|sp| sp.mem_bytes);
        let max_dr = col_max(|sp| sp.disk_read_ewma);
        let max_dw = col_max(|sp| sp.disk_write_ewma);
        let max_disk = col_max(|sp| sp.disk_read_ewma + sp.disk_write_ewma);

        let rows = rows_data.iter().map(move |row| {
            let sp = row.0;
            let prefix = row.1.as_str();
            let mut liveness_style = Style::default();
            if sp.is_dead() {
                liveness_style = liveness_style.fg(Color::Red);
            }
            // freshly-appeared rows get an amber wash on just the name (below),
            // fading to black over their remaining ticks, so newer arrivals are
            // brighter than older ones.
            let flash_bg = hl.flash.get(&sp.pid).map(|&ticks| {
                let t = ticks as f32 / FLASH_TICKS as f32;
                let amber = |v: f32| (v * t).round() as u8;
                Color::Rgb(amber(110.0), amber(90.0), amber(30.0))
            });
            let is_selected = hl.selected == Some(sp.pid);
            let values = vdcols.iter().map(|c| match c.column {
                Pid => Cell::from(Span::styled(sp.pid.to_string(), liveness_style)),
                User => Cell::from(Span::styled(sp.user.clone(), liveness_style)),
                State => {
                    let style = render::state_color(sp.state)
                        .map_or_else(Style::default, |c| Style::default().fg(c));
                    Cell::from(Span::styled(sp.state.to_string(), style))
                }
                ProcessName => {
                    // dim tree-indent prefix (never highlighted), then the name.
                    // the flash wash and selection reverse-video apply only to
                    // the name span so the indent guides stay readable.
                    let mut spans = Vec::new();
                    if !prefix.is_empty() {
                        spans.push(Span::styled(
                            prefix.to_string(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    let mut name_style = liveness_style;
                    if let Some(bg) = flash_bg {
                        name_style = name_style.bg(bg);
                    }
                    if is_selected {
                        name_style = name_style.add_modifier(Modifier::REVERSED);
                    }
                    spans.push(Span::styled(sp.name.clone(), name_style));
                    Cell::from(Line::from(spans))
                }
                Disk => {
                    let v = sp.disk_read_ewma + sp.disk_write_ewma;
                    heat_cell(render_bytes(v), v, max_disk, sp)
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
                // most-recent samples, newest pinned to the right edge (so a
                // given column is the same point in time across every row, and
                // the right edge is always "now"); taller bars add vertical
                // resolution (one Line per row).
                CpuHistory => {
                    let lines: Vec<Line> =
                        render::render_cpu_history(&sp.cpu_hist, hist_w, bar_height as usize)
                            .into_iter()
                            .map(Line::from)
                            .collect();
                    Cell::from(Text::from(lines))
                }
            });
            Row::new(values).height(bar_height)
        });

        // Selection highlighting is applied to the name span (above), not the
        // whole row; TableState still drives auto-scroll to keep it visible.
        Table::new(rows, constraints.iter().copied())
            .header(Row::new(header).style(Style::default().add_modifier(Modifier::UNDERLINED)))
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

    /// A view that ranks by the instant CPU metric, for tests that exercise
    /// ordering by `cpu_ewma` independent of the sustained-rank default.
    fn instant_view() -> View {
        let mut v = View::default();
        v.state.sustained = false;
        v
    }

    // visual aid: cargo test --lib -- --ignored --nocapture core_preview
    #[test]
    #[ignore]
    fn core_preview() {
        // 4 cores at increasing steady levels, plus a ramp on core 0
        let cores = vec![
            vec![5.0, 20.0, 45.0, 70.0, 95.0, 100.0, 60.0, 30.0],
            vec![15.0; 8],
            vec![55.0; 8],
            vec![98.0; 8],
        ];
        println!("\n--- core graphs (double height) ---");
        for line in core_lines(&cores, 240) {
            let s: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
            println!("{s}");
        }
        println!();
    }

    // visual aid: cargo test --lib -- --ignored --nocapture tree_preview
    #[test]
    #[ignore]
    fn tree_preview() {
        // launchd
        //  ├ loginwindow ─ WindowServer(busy)
        //  ├ kernel_task(busy)
        //  └ bash ─ cargo ─ {rustc(busy), rustc(busy)}
        let mk = |pid: u32, name: &str, parent: Option<u32>, cpu: f64| {
            let mut sp = SProc::blank(pid, name);
            sp.parent = parent.map(|p| Pid::from(p as usize));
            sp.cpu_ewma = cpu;
            sp
        };
        let procs = vec![
            mk(1, "launchd", None, 0.0),
            mk(2, "loginwindow", Some(1), 0.0),
            mk(3, "WindowServer", Some(2), 40.0),
            mk(4, "kernel_task", Some(1), 70.0),
            mk(5, "bash", Some(1), 0.0),
            mk(6, "cargo", Some(5), 0.0),
            mk(7, "rustc", Some(6), 95.0),
            mk(8, "rustc", Some(6), 90.0),
        ];
        let refs: Vec<&SProc> = procs.iter().collect();
        let rows = View::default().tree_rows(&refs);
        println!("\n--- reversed tree (leaves top, roots bottom) ---");
        for (sp, prefix) in &rows {
            println!("{prefix}{}  ({:.0}%)", sp.name, sp.cpu_ewma);
        }
        println!();
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
    fn core_lines_grid_is_balanced_and_stable() {
        // each grid row is two lines tall (double-height bars). 8 cores wide ->
        // 4 per row => 2 grid rows => 4 lines.
        let eight: Vec<Vec<f64>> = (0..8).map(|_| vec![10.0]).collect();
        assert_eq!(core_lines(&eight, 240).len(), 4);

        // a single grid row still emits two lines
        let two: Vec<Vec<f64>> = (0..2).map(|_| vec![10.0]).collect();
        assert_eq!(core_lines(&two, 240).len(), 2);

        // layout doesn't change as history fills (1 sample vs full)
        let eight_full: Vec<Vec<f64>> = (0..8).map(|_| vec![10.0; CORE_SPARK_LEN]).collect();
        assert_eq!(
            core_lines(&eight, 240).len(),
            core_lines(&eight_full, 240).len()
        );

        // narrow terminal fits fewer per row -> more grid rows -> more lines
        assert!(core_lines(&eight, 40).len() > 4);

        assert!(core_lines(&[], 80).is_empty());
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
            cores: vec![],
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

        // indent prefix grows with depth (3 box-drawing chars per level)
        let indent = |pid: u32| {
            rows.iter()
                .find(|r| r.0.pid.as_u32() == pid)
                .unwrap()
                .1
                .chars()
                .count()
        };
        assert_eq!(indent(1), 0, "root unindented");
        assert!(indent(2) > 0, "child indented");
        assert!(indent(3) > indent(2), "grandchild deeper");
        assert_eq!(indent(4), 0, "standalone root");
        assert_eq!(indent(5), 0, "orphan becomes root");

        // reversed layout: leaves on top, roots at the bottom, so a child comes
        // *before* its parent
        let pos = |pid: u32| rows.iter().position(|r| r.0.pid.as_u32() == pid).unwrap();
        assert!(pos(3) < pos(2) && pos(2) < pos(1)); // deepest first, root last
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

        // sort Cpu, Desc (instant). Leaves sit on top, roots at the bottom, but
        // the *busiest* branch still ranks to the top: A's subtree (101)
        // outranks B (50), so A's busy child is at the very top, A right below
        // it, then B at the bottom.
        let rows = instant_view().tree_rows(&all);
        let pids: Vec<u32> = rows.iter().map(|r| r.0.pid.as_u32()).collect();
        assert_eq!(pids, vec![2, 1, 3]); // child-of-A (top), A, then B at bottom
    }

    #[test]
    fn tree_reversed_keeps_busiest_on_top_with_leaves_above_parents() {
        // root1(idle) has two leaves: leafA(busy 90), leafB(10).
        // root2(50) stands alone. Subtree totals: root1=100, root2=50.
        let root1 = SProc::blank(1, "root1");
        let mut leaf_a = SProc::blank(2, "leafA");
        leaf_a.cpu_ewma = 90.0;
        leaf_a.parent = Some(Pid::from(1usize));
        let mut leaf_b = SProc::blank(3, "leafB");
        leaf_b.cpu_ewma = 10.0;
        leaf_b.parent = Some(Pid::from(1usize));
        let mut root2 = SProc::blank(4, "root2");
        root2.cpu_ewma = 50.0;
        let all = vec![&root1, &leaf_a, &leaf_b, &root2];

        let rows = instant_view().tree_rows(&all); // Cpu, Desc (instant)
        let pids: Vec<u32> = rows.iter().map(|r| r.0.pid.as_u32()).collect();
        // top -> bottom: busiest leaf, then its sibling, then their parent, then
        // the lower-ranked standalone root at the very bottom.
        assert_eq!(pids, vec![2, 3, 1, 4]);

        // a child is always drawn above its parent
        let pos = |pid: u32| rows.iter().position(|r| r.0.pid.as_u32() == pid).unwrap();
        assert!(
            pos(2) < pos(1) && pos(3) < pos(1),
            "leaves above their parent"
        );

        // connectors point upward (top corners ╭, never bottom corners ╰)
        let prefixes: String = rows.iter().map(|r| r.1.as_str()).collect();
        assert!(prefixes.contains('╭'), "uses upward top-corner glyph");
        assert!(!prefixes.contains('╰'), "no leftover bottom-corner glyph");
    }

    #[test]
    fn tree_reversed_groups_leaf_children_against_parent() {
        // root R has a leaf child L and a subtree child M(->grandchild G). The
        // leaf must sit directly against R, and M's subtree must stay contiguous
        // — L must never wedge between M and G ("branch split by another").
        let r = SProc::blank(1, "R");
        let mut l = SProc::blank(2, "L");
        l.parent = Some(Pid::from(1usize));
        let mut m = SProc::blank(3, "M");
        m.parent = Some(Pid::from(1usize));
        let mut g = SProc::blank(4, "G");
        g.parent = Some(Pid::from(3usize));
        let all = vec![&r, &l, &m, &g];

        let rows = View::default().tree_rows(&all);
        let pos = |pid: u32| rows.iter().position(|r| r.0.pid.as_u32() == pid).unwrap();

        // root at the very bottom; its leaf child directly above it
        assert_eq!(pos(1), rows.len() - 1, "root at bottom");
        assert_eq!(pos(2) + 1, pos(1), "leaf child sits directly against root");
        // the subtree child and its grandchild stay adjacent (branch intact),
        // with the leaf not interleaved between them
        assert_eq!(pos(4) + 1, pos(3), "grandchild directly above its parent");
        assert!(
            pos(3) < pos(2),
            "intact subtree stacks above the leaf group"
        );
    }

    #[test]
    fn aggregate_sums_same_named_processes() {
        let mut a = SProc::blank(10, "chrome");
        a.cpu_ewma = 5.0;
        a.mem_bytes = 100.0;
        a.cpu_hist = [1.0, 2.0].into();
        let mut b = SProc::blank(3, "chrome");
        b.cpu_ewma = 7.0;
        b.mem_bytes = 50.0;
        b.cpu_hist = [10.0].into();
        let c = SProc::blank(4, "bash");
        let agg = aggregate_by_name(&[&a, &b, &c]);

        assert_eq!(agg.len(), 2); // chrome group + bash
        let chrome = agg.iter().find(|s| s.name.starts_with("chrome")).unwrap();
        assert_eq!(chrome.name, "chrome (2)");
        assert_eq!(chrome.cpu_ewma, 12.0);
        assert_eq!(chrome.mem_bytes, 150.0);
        assert_eq!(chrome.pid.as_u32(), 3); // group id = lowest pid
                                            // histories summed element-wise (newest-first): [1,2] + [10] = [11,2]
        assert_eq!(
            chrome.cpu_hist.iter().copied().collect::<Vec<_>>(),
            vec![11.0, 2.0]
        );
    }

    #[test]
    fn aggregate_key_collapses_helper_and_role_suffixes() {
        assert_eq!(aggregate_key("Google Chrome"), "Google Chrome");
        assert_eq!(aggregate_key("Google Chrome Helper"), "Google Chrome");
        assert_eq!(
            aggregate_key("Google Chrome Helper (Renderer)"),
            "Google Chrome"
        );
        assert_eq!(aggregate_key("Google Chrome Helper (GPU)"), "Google Chrome");
        // unrelated names are untouched
        assert_eq!(aggregate_key("bash"), "bash");
    }

    #[test]
    fn aggregate_groups_chrome_family_under_one_row() {
        let mut main = SProc::blank(1, "Google Chrome");
        main.cpu_ewma = 3.0;
        let mut helper = SProc::blank(2, "Google Chrome Helper");
        helper.cpu_ewma = 5.0;
        let mut renderer = SProc::blank(3, "Google Chrome Helper (Renderer)");
        renderer.cpu_ewma = 11.0;
        let gpu = SProc::blank(4, "Google Chrome Helper (GPU)");
        let bash = SProc::blank(5, "bash");
        let agg = aggregate_by_name(&[&main, &helper, &renderer, &gpu, &bash]);

        assert_eq!(agg.len(), 2); // whole chrome family + bash
        let chrome = agg
            .iter()
            .find(|s| s.name.starts_with("Google Chrome"))
            .unwrap();
        assert_eq!(chrome.name, "Google Chrome (4)");
        assert_eq!(chrome.cpu_ewma, 19.0); // 3 + 5 + 11 + 0
        assert_eq!(chrome.pid.as_u32(), 1); // lowest pid (the main process)
    }

    #[test]
    fn new_rows_flash_then_fade() {
        let pids = |ids: &[u32]| {
            ids.iter()
                .map(|&i| Pid::from(i as usize))
                .collect::<HashSet<_>>()
        };
        let mut v = View::default();

        v.note_new_rows(&pids(&[1, 2]));
        assert!(v.flash.is_empty(), "first population doesn't flash");

        v.note_new_rows(&pids(&[1, 2, 3]));
        assert!(v.flash.contains_key(&Pid::from(3usize)), "new pid flashes");
        assert!(
            !v.flash.contains_key(&Pid::from(1usize)),
            "existing pid doesn't"
        );

        for _ in 0..FLASH_TICKS {
            v.fade_flashes();
        }
        assert!(v.flash.is_empty(), "flash fades after FLASH_TICKS");
    }

    #[test]
    fn flat_order_freezes_until_membership_or_sort_changes() {
        let mut a = SProc::blank(1, "a");
        a.cpu_ewma = 10.0;
        let mut b = SProc::blank(2, "b");
        b.cpu_ewma = 5.0;
        // freezing only applies in instant mode; sustained mode intentionally
        // re-sorts every tick (its metric is smooth).
        let mut v = instant_view(); // Cpu, Desc (instant)

        let order =
            |rows: &[(&SProc, String)]| rows.iter().map(|r| r.0.pid.as_u32()).collect::<Vec<_>>();

        // initial: a(10) above b(5)
        let rows = v.flat_rows(&mut vec![&a, &b]);
        assert_eq!(order(&rows), vec![1, 2]);

        // a drops below b but same set+sort -> order stays frozen
        let mut a_low = SProc::blank(1, "a");
        a_low.cpu_ewma = 1.0;
        let rows = v.flat_rows(&mut vec![&b, &a_low]); // input order shouldn't matter
        assert_eq!(
            order(&rows),
            vec![1, 2],
            "frozen while membership unchanged"
        );

        // adding a process changes the set -> re-sort by value
        let mut c = SProc::blank(3, "c");
        c.cpu_ewma = 8.0;
        let rows = v.flat_rows(&mut vec![&a_low, &b, &c]);
        assert_eq!(order(&rows), vec![3, 2, 1], "re-sorts on membership change");
    }

    #[test]
    fn sustained_ranking_orders_by_slow_rank_not_instant_spike() {
        // spiker is high right now but has no sustained history; steady is lower
        // right now but has built up a high sustained rank.
        let mut spiker = SProc::blank(1, "spiker");
        spiker.cpu_ewma = 95.0;
        spiker.cpu_rank = 5.0;
        let mut steady = SProc::blank(2, "steady");
        steady.cpu_ewma = 40.0;
        steady.cpu_rank = 38.0;

        let order =
            |rows: &[(&SProc, String)]| rows.iter().map(|r| r.0.pid.as_u32()).collect::<Vec<_>>();

        // default (sustained on): steady outranks the fresh spike
        let mut v = View::default();
        let rows = v.flat_rows(&mut vec![&spiker, &steady]);
        assert_eq!(
            order(&rows),
            vec![2, 1],
            "sustained: steady above fresh spike"
        );

        // toggling sustained off ranks by the instant value -> spiker jumps up.
        // (also verifies the toggle forces a re-sort despite the frozen order)
        v.state.sustained = false;
        let rows = v.flat_rows(&mut vec![&spiker, &steady]);
        assert_eq!(order(&rows), vec![1, 2], "instant: fresh spike on top");
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

    #[test]
    fn recently_active_process_lingers_before_hiding() {
        let mut v = View::default(); // hide_idle on by default
        let busy = proc_with_cpu(1, 50.0);
        let idle = proc_with_cpu(2, 0.0);

        // while active it's shown; the always-idle one is hidden
        v.update_keep_alive(&[&busy, &idle]);
        let vis = v.visible(&[&busy, &idle]);
        assert!(vis.iter().any(|s| s.pid == busy.pid));
        assert!(!vis.iter().any(|s| s.pid == idle.pid));

        // it dips to idle -> must linger, not vanish on the very next tick
        let now_idle = proc_with_cpu(1, 0.0);
        v.update_keep_alive(&[&now_idle, &idle]);
        assert!(
            v.visible(&[&now_idle, &idle])
                .iter()
                .any(|s| s.pid == now_idle.pid),
            "recently-active process lingers instead of disappearing next tick"
        );

        // ...and never flickers back out while it keeps getting brief spikes
        for _ in 0..KEEP_ALIVE_TICKS * 2 {
            let spike = proc_with_cpu(1, 50.0);
            v.update_keep_alive(&[&spike, &idle]);
            v.update_keep_alive(&[&now_idle, &idle]); // idle tick in between
            assert!(
                v.visible(&[&now_idle, &idle])
                    .iter()
                    .any(|s| s.pid == now_idle.pid),
                "a process spiking within the grace window stays put"
            );
        }

        // once it stays idle past the grace window it finally drops out
        for _ in 0..KEEP_ALIVE_TICKS {
            v.update_keep_alive(&[&now_idle, &idle]);
        }
        assert!(
            !v.visible(&[&now_idle, &idle])
                .iter()
                .any(|s| s.pid == now_idle.pid),
            "hidden once the grace window expires"
        );
    }

    #[test]
    fn tree_prunes_to_active_branches_with_ancestors() {
        // init(idle) -> shell(idle) -> worker(busy); plus a lonely idle proc
        let init = SProc::blank(1, "init");
        let mut shell = SProc::blank(2, "shell");
        shell.parent = Some(Pid::from(1usize));
        let mut worker = SProc::blank(3, "worker");
        worker.parent = Some(Pid::from(2usize));
        worker.cpu_ewma = 20.0;
        let lonely = SProc::blank(4, "lonely");
        let all = vec![&init, &shell, &worker, &lonely];

        let mut v = View::default();
        v.state.tree = true; // hide_idle on by default
        let pids: HashSet<u32> = v.visible(&all).iter().map(|s| s.pid.as_u32()).collect();
        assert!(pids.contains(&3), "active worker kept");
        assert!(
            pids.contains(&2) && pids.contains(&1),
            "ancestors kept for context"
        );
        assert!(!pids.contains(&4), "lonely idle branch pruned");

        // show-all expands to the whole tree
        v.state.hide_idle = false;
        assert_eq!(v.visible(&all).len(), 4);
    }
}
