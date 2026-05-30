/// Detail: full-screen drill-down for a single process, with high-resolution
/// braille line charts of its CPU, memory and disk-I/O history.
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Frame,
};

use crate::render::human_bytes;
use crate::sproc::SProc;

/// One series to plot: legend name, line color, and (x, y) points.
type Series<'a> = (&'a str, Color, &'a [(f64, f64)]);

// Below this height, three stacked charts get too short to read, so we lay
// them side-by-side instead -- provided the pane is wide enough to split.
const MIN_STACK_HEIGHT: u16 = 24;
const MIN_SIDE_BY_SIDE_WIDTH: u16 = 90;

/// Short, wide panes read better with the charts side-by-side (each keeps full
/// height) than squished into a tall stack.
fn use_horizontal(area: Rect) -> bool {
    area.height < MIN_STACK_HEIGHT && area.width >= MIN_SIDE_BY_SIDE_WIDTH
}

pub fn render_detail(f: &mut Frame, area: Rect, sp: &SProc, secs_per_sample: f64) {
    let rows = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Min(0),    // charts
    ])
    .split(area);

    render_header(f, rows[0], sp);

    let thirds = [
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ];
    let charts = if use_horizontal(area) {
        Layout::horizontal(thirds).split(rows[1])
    } else {
        Layout::vertical(thirds).split(rows[1])
    };

    // history is newest-first; reverse so time flows left (oldest) -> right (now)
    let cpu = points(sp.cpu_hist.iter().rev().copied());
    let mem = points(sp.mem_hist.iter().rev().copied());
    let read = points(sp.disk_read_hist.iter().rev().map(|&b| b as f64));
    let write = points(sp.disk_write_hist.iter().rev().map(|&b| b as f64));

    render_chart(
        f,
        charts[0],
        "CPU %",
        &[("cpu", Color::Cyan, &cpu)],
        secs_per_sample,
        |v| format!("{:.0}%", v),
    );
    render_chart(
        f,
        charts[1],
        "Memory",
        &[("mem", Color::Green, &mem)],
        secs_per_sample,
        |v| format!("{:.0}MB", v),
    );
    render_chart(
        f,
        charts[2],
        "Disk I/O (bytes/sample)",
        &[
            ("read", Color::Blue, &read),
            ("write", Color::Magenta, &write),
        ],
        secs_per_sample,
        human_bytes,
    );
}

fn render_header(f: &mut Frame, area: Rect, sp: &SProc) {
    let mut name = Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED);
    if sp.is_dead() {
        name = name.fg(Color::Red);
    }
    let header = Line::from(vec![
        Span::styled(format!(" {} ", sp.name), name),
        Span::raw(format!(
            "  pid {}   cpu {:.1}%   mem {:.1}MB",
            sp.pid, sp.cpu_ewma, sp.mem_mb
        )),
    ]);
    f.render_widget(Paragraph::new(header), area);
}

fn points(values: impl Iterator<Item = f64>) -> Vec<(f64, f64)> {
    values.enumerate().map(|(i, v)| (i as f64, v)).collect()
}

fn render_chart(
    f: &mut Frame,
    area: Rect,
    title: &str,
    series: &[Series],
    secs_per_sample: f64,
    fmt_y: impl Fn(f64) -> String,
) {
    let n = series.iter().map(|(_, _, p)| p.len()).max().unwrap_or(0);
    // a line needs at least two points; until then just show the empty frame
    if n < 2 {
        f.render_widget(
            Block::default()
                .title(title.to_string())
                .borders(Borders::ALL),
            area,
        );
        return;
    }

    let x_max = (n - 1) as f64;
    // headroom above the peak so the line isn't glued to the top border
    let y_max = series
        .iter()
        .flat_map(|(_, _, p)| p.iter().map(|&(_, y)| y))
        .fold(0.0_f64, f64::max)
        .max(1.0)
        * 1.15;

    let datasets: Vec<Dataset> = series
        .iter()
        .map(|(name, color, pts)| {
            Dataset::default()
                .name(*name)
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(*color))
                .data(pts)
        })
        .collect();

    let x_axis = Axis::default().bounds([0.0, x_max]).labels([
        Line::from(format!("-{:.0}s", x_max * secs_per_sample)),
        Line::from("now"),
    ]);
    let y_axis = Axis::default()
        .bounds([0.0, y_max])
        .labels([Line::from("0"), Line::from(fmt_y(y_max))]);

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(title.to_string())
                .borders(Borders::ALL),
        )
        .x_axis(x_axis)
        .y_axis(y_axis)
        // the default constraints hide the legend in short panes; show it
        // whenever it physically fits so the read/write series stay labelled
        .hidden_legend_constraints((Constraint::Percentage(100), Constraint::Percentage(100)));
    f.render_widget(chart, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    /// Flatten a rendered detail screen into one string of glyphs (row-major),
    /// so we can assert on content without scraping terminal escape codes.
    fn render_to_text(sp: &SProc, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| render_detail(f, f.area(), sp, 1.0))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn sample_proc() -> SProc {
        let mut sp = SProc::blank(42, "testproc");
        for i in 0..30u64 {
            sp.cpu_hist.push_front(i as f64);
            sp.mem_hist.push_front((i * 10) as f64);
            sp.disk_read_hist.push_front(i * 1024);
            sp.disk_write_hist.push_front(i * 512);
        }
        sp
    }

    #[test]
    fn detail_renders_header_and_three_charts() {
        let text = render_to_text(&sample_proc(), 120, 30);
        assert!(text.contains("testproc"), "process name missing");
        assert!(text.contains("CPU %"), "cpu chart title missing");
        assert!(text.contains("Memory"), "memory chart title missing");
        assert!(text.contains("Disk I/O"), "disk chart title missing");
        assert!(
            text.contains("read") && text.contains("write"),
            "disk legend missing"
        );
        assert!(text.contains("now"), "time axis label missing");
        // high-res braille line glyphs should be present
        assert!(text.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c)));
    }

    #[test]
    #[ignore = "visual preview; run with --ignored --nocapture"]
    fn preview() {
        let mut sp = SProc::blank(4242, "firefox");
        for i in 0..40u64 {
            let cpu = 40.0 + 35.0 * ((i as f64) * 0.5).sin();
            sp.cpu_hist.push_front(cpu.max(0.0));
            sp.mem_hist.push_front(200.0 + i as f64 * 8.0);
            sp.disk_read_hist
                .push_front(if i % 5 == 0 { 1_500_000 } else { 2000 });
            sp.disk_write_hist
                .push_front(if i % 7 == 0 { 800_000 } else { 500 });
        }
        sp.cpu_ewma = 52.3;
        sp.mem_mb = 512.0;
        for (w, h, label) in [(110, 32, "tall (vertical)"), (150, 14, "short & wide")] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| render_detail(f, f.area(), &sp, 1.0)).unwrap();
            println!("\n=== {label} {w}x{h} ===");
            print!("{}", t.backend());
        }
    }

    #[test]
    fn detail_handles_empty_history_without_panicking() {
        // a brand-new process with <2 samples: charts fall back to empty frames
        let sp = SProc::blank(1, "fresh");
        let text = render_to_text(&sp, 120, 30);
        assert!(text.contains("fresh"));
        assert!(text.contains("CPU %"));
    }

    #[test]
    fn layout_is_horizontal_only_when_short_and_wide() {
        assert!(use_horizontal(Rect::new(0, 0, 200, 12))); // short & wide
        assert!(!use_horizontal(Rect::new(0, 0, 200, 40))); // tall enough: stack
        assert!(!use_horizontal(Rect::new(0, 0, 50, 12))); // too narrow to split
    }

    #[test]
    fn short_wide_window_still_shows_all_three_charts() {
        let text = render_to_text(&sample_proc(), 160, 13);
        assert!(text.contains("CPU %"));
        assert!(text.contains("Memory"));
        assert!(text.contains("Disk I/O"));
    }
}
