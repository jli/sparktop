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
pub fn render_vec_colored_multi<'a, II>(xs: II, max: f64, height: usize) -> Vec<Vec<Span<'a>>>
where
    II: IntoIterator<Item = &'a f64>,
{
    let height = height.max(1);
    let mut rows: Vec<Vec<Span>> = (0..height).map(|_| Vec::new()).collect();
    for x in xs {
        let p = *x / max; // 1.0 == one full row; not clamped, so >100% stacks up
        let color = cpu_color(*x);
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
fn float_bar(mut f: f64) -> char {
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
        let multi = render_vec_colored_multi(hist.iter(), 100., 1);
        assert_eq!(multi.len(), 1);
        let multi_syms: Vec<_> = multi[0].iter().map(|s| s.content.to_string()).collect();
        let single_syms: Vec<_> = single.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(multi_syms, single_syms);
    }

    #[test]
    fn multi_height_one_full_line_per_100_percent() {
        // rows[0] is the top, rows[2] the bottom. Each row == 100%.
        // 100% fills only the bottom row...
        let rows = render_vec_colored_multi([100.0].iter(), 100., 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2][0].content.as_ref(), "█"); // bottom full
        assert_eq!(rows[1][0].content.as_ref(), " "); // middle empty
        assert_eq!(rows[0][0].content.as_ref(), " "); // top empty

        // ...300% fills all three rows (visible multi-core usage)
        let rows = render_vec_colored_multi([300.0].iter(), 100., 3);
        assert!(rows.iter().all(|r| r[0].content.as_ref() == "█"));

        // a zero sample fills nothing
        let rows = render_vec_colored_multi([0.0].iter(), 100., 3);
        assert!(rows.iter().all(|r| r[0].content.as_ref() == " "));
    }

    #[test]
    fn human_bytes_scales_units() {
        assert_eq!(human_bytes(512.0), "512B");
        assert_eq!(human_bytes(1536.0), "1.5KB");
        assert_eq!(human_bytes(5.0 * 1024.0 * 1024.0), "5.0MB");
    }

    #[test]
    fn cpu_color_thresholds() {
        assert_eq!(cpu_color(50.0), None);
        assert_eq!(cpu_color(150.0), Some(Color::Red));
        assert_eq!(cpu_color(250.0), Some(Color::LightMagenta));
        assert_eq!(cpu_color(500.0), Some(Color::Magenta));
    }
}
