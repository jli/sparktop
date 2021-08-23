/// View: rendering the UI, interactions.
use anyhow::Result;
use ordered_float::OrderedFloat as OrdFloat;

use crossterm::event::KeyEvent;
use tui::{
    layout::{Alignment, Constraint, Layout},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph, Row, Table},
};

use crate::{
    event::Next,
    sproc::SProc,
    sterm::STerm,
    view_state::{Dir, DisplayedColumns, SortColumn, ViewDisplayColumn, ViewState},
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
                SortColumn::Pid => sp.pid as f64,
                SortColumn::Cpu => sp.cpu_ewma,
                SortColumn::Mem => sp.mem_mb,
                SortColumn::DiskRead => sp.disk_read_ewma,
                SortColumn::DiskWrite => sp.disk_write_ewma,
                SortColumn::DiskTotal => sp.disk_read_ewma + sp.disk_write_ewma,
            };
            match self.state.sort_dir {
                Dir::Asc => OrdFloat(val),
                Dir::Desc => OrdFloat(-val),
            }
        });
    }

    // ViewState + KeyEvent -> Option<Action>
    // ViewState + Action -> ViewState
    pub fn handle_key(&mut self, key: KeyEvent) -> Next {
        // let mut next = Next::Continue;
        self.state.handle_key(key);
        if self.state.should_quit {
            Next::Quit
        } else {
            Next::Continue
        }
        // match key.code {
        //     KeyCode::Char('N') => self.state.sort_by = Metric::Pid,
        //     KeyCode::Char('M') => self.state.sort_by = Metric::Mem,
        //     KeyCode::Char('P') => self.state.sort_by = Metric::Cpu,
        //     KeyCode::Char('R') => self.state.sort_by = Metric::DiskRead,
        //     KeyCode::Char('W') => self.state.sort_by = Metric::DiskWrite,
        //     KeyCode::Char('D') => self.state.sort_by = Metric::DiskTotal,
        //     KeyCode::Char('I') => self.state.sort_dir.flip(),
        //     KeyCode::Char('q') => next = Next::Quit,
        //     KeyCode::Esc => (), // clear alert
        //     _ => unhandled = true,
        // }

        // next
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
            // TODO: lifetime hacks...
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

impl ProcTable {
    fn build<'a>(
        sprocs: &'a [&SProc],
        sort_by: SortColumn,
        display_columns: DisplayedColumns,
        constraints: &'a [Constraint],
    ) -> impl tui::widgets::Widget + 'a {
        // use DisplayColumn::*;
        let header = display_columns.header(&sort_by);
        // let vdcols = display_columns.shown();
        let rows = sprocs.iter().map(|sp| {
            // let mut liveness_style = Style::default();
            // if sp.is_dead() {
            //     liveness_style = liveness_style.fg(Color::Red);
            // }
            // let values = vdcols.map(|ViewDisplayColumn(c, _, _, _)| match c {
            //     Pid => Span::styled(sp.pid.to_string(), liveness_style),
            //     ProcessName => Span::styled(sp.name.clone(), liveness_style),
            //     DiskRead => Span::from(render_metric(sp.disk_read_ewma)),
            //     DiskWrite => Span::from(render_metric(sp.disk_write_ewma)),
            //     Mem => Span::from(render_metric(sp.mem_mb)),
            //     Cpu => Span::from(render_metric(sp.cpu_ewma)),
            //     CpuHistory => Span::from(render::render_vec(&sp.cpu_hist, 100.)),
            // });
            let values = display_columns.row_data(sp);
            Row::new(values)
        });
        // let constraints: Vec<Constraint> = vdcols.iter().map(|ViewDisplayColumn(_,_,_,_,constraint)| constraint).copied().collect();
        Table::new(rows)
            .header(
                // TODO: way to avoid making copy?
                Row::new(header.to_vec())
                    .style(Style::default().add_modifier(Modifier::UNDERLINED)),
            )
            .widths(constraints)
        // .widths(&[
        //     Constraint::Length(6),
        //     Constraint::Length(24),
        //     Constraint::Length(5),
        //     Constraint::Length(5),
        //     Constraint::Length(5),
        //     Constraint::Length(4),
        //     Constraint::Percentage(100),
        // ])
    }
    // fn new(sprocs: &'a [&SProc], sort_by: SortColumn, display_columns: DisplayedColumns) -> Self {
    //     // use SortColumn as S;
    //     // let mut header = vec![String::from("pid"), String::from("process")];
    //     // header.extend(
    //     //     [S.DiskRead, S.DiskWrite, S.Mem, S.Cpu]
    //     //         .iter()
    //     //         .map(|m| m.to_header_str(sort_by)),
    //     // );
    //     // header.push(String::from("cpu history"));
    //     // Self { header, sprocs }
    //     let header = display_columns.header(&sort_by);
    //     Self { header, sprocs }
    // }

    // fn get_table(&self) -> impl tui::widgets::Widget + '_ {
    //     let rows = self.sprocs.iter().map(|sp| {
    //         let mut liveness_style = Style::default();
    //         if sp.is_dead() {
    //             liveness_style = liveness_style.fg(Color::Red);
    //         }
    //         Row::new(
    //             vec![
    //                 Span::styled(sp.pid.to_string(), liveness_style),
    //                 Span::styled(sp.name.clone(), liveness_style),
    //                 Span::from(render_metric(sp.disk_read_ewma)),
    //                 Span::from(render_metric(sp.disk_write_ewma)),
    //                 Span::from(render_metric(sp.mem_mb)),
    //                 Span::from(render_metric(sp.cpu_ewma)),
    //                 Span::from(render::render_vec(&sp.cpu_hist, 100.)),
    //             ]
    //             .into_iter(),
    //         )
    //     });
    //     Table::new(rows)
    //         .header(
    //             // TODO: way to avoid making copy?
    //             Row::new(self.header.to_vec())
    //                 .style(Style::default().add_modifier(Modifier::UNDERLINED)),
    //         )
    //         .widths(&[
    //             Constraint::Length(6),
    //             Constraint::Length(24),
    //             Constraint::Length(5),
    //             Constraint::Length(5),
    //             Constraint::Length(5),
    //             Constraint::Length(4),
    //             Constraint::Percentage(100),
    //         ])
    // }
}

// impl SortColumn {
//     fn to_header_str(self, sort_by: SortColumn) -> String {
//         use SortColumn::*;
//         let s = match self {
//             Pid => "pid",
//             Cpu => "cpu",
//             Mem => "mem",
//             DiskRead => "dr",
//             DiskWrite => "dw",
//             DiskTotal => "d+",
//         };
//         if sort_by == self || (sort_by == DiskTotal && (self == DiskRead || self == DiskWrite)) {
//             format!("*{}*", s)
//         } else {
//             String::from(s)
//         }
//     }
// }

// hide low values
// fn render_metric(m: f64) -> String {
//     if m < 0.05 {
//         String::from("_")
//     } else {
//         format!("{:.1}", m)
//     }
// }
