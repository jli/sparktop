// Rendering logic.

pub fn render_vec(xs: &Vec<f64>, max: f64) -> String {
    let mut r = String::new();
    for x in xs {
        let p = *x / max;
        r.push(float_bar(p));
    }
    r
}

const BARS: [char; 8]  = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// f must be between 0 and 1.
fn float_bar(mut f: f64) -> char {
    f = f.min(1.);  // cpu usage can be > 1. do something special?
    if f < 0.03 { return ' ' }
    let sub_seg = 1. / BARS.len() as f64;
    let mut i = 0;
    while f > sub_seg {
        f -= sub_seg;
        i += 1;
    }
    BARS[i]
}
