/// Rendering logic.
use tui::{
    style::{Color, Style},
    text::Span,
};

pub fn render_vec<'a, II>(xs: II, max: f64) -> String
where
    II: IntoIterator<Item = &'a f64>,
{
    let mut r = String::new();
    for x in xs.into_iter() {
        let p = *x / max;
        r.push(float_bar(p));
    }
    r
}

pub fn render_vec_colored<'a, II>(xs: II, max: f64) -> Vec<Span<'a>>
where
    II: IntoIterator<Item = &'a f64>,
{
    let mut result = Vec::new();
    for x in xs.into_iter() {
        let p = *x / max;
        let c = float_bar(p);
        let color = cpu_color(*x);
        if let Some(color) = color {
            result.push(Span::styled(c.to_string(), Style::default().fg(color)));
        } else {
            result.push(Span::raw(c.to_string()));
        }
    }
    result
}

fn cpu_color(cpu: f64) -> Option<Color> {
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
