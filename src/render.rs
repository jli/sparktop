/// Rendering logic: turning metric histories into colored sparklines.
use ratatui::{
    style::{Color, Style},
    text::Span,
};

pub fn render_vec_colored<'a, II>(xs: II, max: f64) -> Vec<Span<'a>>
where
    II: IntoIterator<Item = &'a f64>,
{
    let mut result = Vec::new();
    for x in xs.into_iter() {
        let p = *x / max;
        let c = float_bar(p);
        match cpu_color(*x) {
            Some(color) => result.push(Span::styled(c.to_string(), Style::default().fg(color))),
            None => result.push(Span::raw(c.to_string())),
        }
    }
    result
}

/// Format a duration in seconds compactly (e.g. "3d4h", "5h2m", "12m").
pub fn fmt_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d{h}h")
    } else if h > 0 {
        format!("{h}h{m}m")
    } else {
        format!("{m}m")
    }
}

/// Format a byte count with a binary unit suffix (e.g. 1536.0 -> "1.5KB").
pub fn human_bytes(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{:.0}{}", v, UNITS[i])
    } else {
        format!("{:.1}{}", v, UNITS[i])
    }
}

/// Like `render_vec_colored`, but spreads each bar across `height` rows. Each
/// row represents one full `max` (i.e. 100%), so a value of `max` fills exactly
/// one line and values past `max` stack into the rows above -- making >100%
/// (multi-core) usage visible. Returns one span-row per output line, top first.
pub fn render_vec_colored_multi<II>(xs: II, max: f64, height: usize) -> Vec<Vec<Span<'static>>>
where
    II: IntoIterator<Item = f64>,
{
    let height = height.max(1);
    let mut rows: Vec<Vec<Span>> = (0..height).map(|_| Vec::new()).collect();
    for x in xs {
        let p = x / max; // 1.0 == one full row; not clamped, so >100% stacks up
        let color = cpu_color(x);
        for (r, row) in rows.iter_mut().enumerate() {
            // band 0 is the bottom row; r == 0 is the top row
            let band = (height - 1 - r) as f64;
            let local = (p - band).clamp(0.0, 1.0);
            let ch = float_bar(local).to_string();
            row.push(match color {
                Some(c) => Span::styled(ch, Style::default().fg(c)),
                None => Span::raw(ch),
            });
        }
    }
    rows
}

/// Map `t` in [0, 1] to a low->high heat color: green -> yellow -> red.
/// Used to shade numeric columns so big values stand out at a glance.
pub fn heat(t: f64) -> Color {
    const LOW: (u8, u8, u8) = (100, 170, 100); // muted green
    const MID: (u8, u8, u8) = (225, 205, 75); // yellow
    const HIGH: (u8, u8, u8) = (235, 75, 65); // red
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8, f: f64| (a as f64 + (b as f64 - a as f64) * f).round() as u8;
    let (from, to, f) = if t < 0.5 {
        (LOW, MID, t / 0.5)
    } else {
        (MID, HIGH, (t - 0.5) / 0.5)
    };
    Color::Rgb(
        lerp(from.0, to.0, f),
        lerp(from.1, to.1, f),
        lerp(from.2, to.2, f),
    )
}

/// Human-readable description of a single-char process state code.
pub fn state_label(state: char) -> &'static str {
    match state {
        'R' => "running",
        'S' => "sleeping",
        'I' => "idle",
        'D' => "uninterruptible sleep (waiting on I/O)",
        'T' => "stopped",
        't' => "traced / debugged",
        'Z' => "zombie (defunct, waiting to be reaped)",
        'X' => "dead",
        'P' => "parked",
        'W' => "waking",
        'L' => "blocked on a lock",
        _ => "unknown",
    }
}

/// Accent color for a process state (None = default foreground), shared by the
/// list's state column and the detail view so they stay consistent.
pub fn state_color(state: char) -> Option<Color> {
    match state {
        'R' => Some(Color::Green),
        'D' | 'Z' => Some(Color::Red),
        'X' => Some(Color::DarkGray),
        _ => None,
    }
}

pub fn cpu_color(cpu: f64) -> Option<Color> {
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

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// f must be between 0 and 1.
pub fn float_bar(mut f: f64) -> char {
    f = f.min(1.); // cpu usage can be > 1. do something special?
    if f < 0.03 {
        return ' ';
    }
    let sub_seg = 1. / BARS.len() as f64;
    let mut i = 0;
    while f > sub_seg {
        f -= sub_seg;
        i += 1;
    }
    BARS[i]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_bar_blank_when_tiny() {
        assert_eq!(float_bar(0.0), ' ');
        assert_eq!(float_bar(0.02), ' ');
    }

    #[test]
    fn float_bar_full_when_saturated() {
        assert_eq!(float_bar(1.0), '█');
        // values over 1.0 are clamped, not out-of-bounds
        assert_eq!(float_bar(5.0), '█');
    }

    #[test]
    fn render_vec_maps_each_sample_to_one_bar() {
        let hist = [0.0, 50.0, 100.0];
        assert_eq!(render_vec_colored(hist.iter(), 100.).len(), 3);
    }

    #[test]
    fn multi_height_matches_single_at_height_one() {
        let hist = [0.0, 50.0, 100.0];
        let single = render_vec_colored(hist.iter(), 100.);
        let multi = render_vec_colored_multi(hist.iter().copied(), 100., 1);
        assert_eq!(multi.len(), 1);
        let multi_syms: Vec<_> = multi[0].iter().map(|s| s.content.to_string()).collect();
        let single_syms: Vec<_> = single.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(multi_syms, single_syms);
    }

    #[test]
    fn multi_height_one_full_line_per_100_percent() {
        // rows[0] is the top, rows[2] the bottom. Each row == 100%.
        // 100% fills only the bottom row...
        let rows = render_vec_colored_multi([100.0].iter().copied(), 100., 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2][0].content.as_ref(), "█"); // bottom full
        assert_eq!(rows[1][0].content.as_ref(), " "); // middle empty
        assert_eq!(rows[0][0].content.as_ref(), " "); // top empty

        // ...300% fills all three rows (visible multi-core usage)
        let rows = render_vec_colored_multi([300.0].iter().copied(), 100., 3);
        assert!(rows.iter().all(|r| r[0].content.as_ref() == "█"));

        // a zero sample fills nothing
        let rows = render_vec_colored_multi([0.0].iter().copied(), 100., 3);
        assert!(rows.iter().all(|r| r[0].content.as_ref() == " "));
    }

    #[test]
    fn heat_ramps_green_to_red() {
        // low: green dominates; high: red dominates; mid: warm (low blue)
        match heat(0.0) {
            Color::Rgb(r, g, _) => assert!(g > r),
            c => panic!("expected rgb, got {:?}", c),
        }
        match heat(1.0) {
            Color::Rgb(r, g, _) => assert!(r > g),
            c => panic!("expected rgb, got {:?}", c),
        }
        match heat(0.5) {
            Color::Rgb(r, g, b) => assert!(r > b && g > b),
            c => panic!("expected rgb, got {:?}", c),
        }
        // clamps out-of-range
        assert_eq!(heat(-1.0), heat(0.0));
        assert_eq!(heat(2.0), heat(1.0));
    }

    #[test]
    fn fmt_uptime_formats() {
        assert_eq!(fmt_uptime(90_061), "1d1h");
        assert_eq!(fmt_uptime(3_700), "1h1m");
        assert_eq!(fmt_uptime(120), "2m");
    }

    #[test]
    fn human_bytes_scales_units() {
        assert_eq!(human_bytes(512.0), "512B");
        assert_eq!(human_bytes(1536.0), "1.5KB");
        assert_eq!(human_bytes(5.0 * 1024.0 * 1024.0), "5.0MB");
    }

    #[test]
    fn state_label_is_friendly() {
        assert_eq!(state_label('R'), "running");
        assert_eq!(state_label('Z'), "zombie (defunct, waiting to be reaped)");
        assert_eq!(state_label('D'), "uninterruptible sleep (waiting on I/O)");
        assert_eq!(state_label('?'), "unknown");
    }

    #[test]
    fn cpu_color_thresholds() {
        assert_eq!(cpu_color(50.0), None);
        assert_eq!(cpu_color(150.0), Some(Color::Red));
        assert_eq!(cpu_color(250.0), Some(Color::LightMagenta));
        assert_eq!(cpu_color(500.0), Some(Color::Magenta));
    }
}
