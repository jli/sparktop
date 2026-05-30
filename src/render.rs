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
