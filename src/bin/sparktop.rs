use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use sparktop::{sprocs::SProcs, view::View};

#[derive(Parser)]
struct Opt {
    /// seconds between samples
    #[arg(short, default_value_t = 1.0, value_parser = parse_delay)]
    delay: f64,
    /// weight given to new samples, in (0, 1]
    #[arg(short, default_value_t = 0.5, value_parser = parse_ewma_weight)]
    ewma_weight: f64,
}

fn parse_delay(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|e| format!("{e}"))?;
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        // zero would busy-loop; Duration::from_secs_f64 panics on negatives
        Err("must be a positive number of seconds".to_string())
    }
}

fn parse_ewma_weight(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|e| format!("{e}"))?;
    if v > 0.0 && v <= 1.0 {
        Ok(v)
    } else {
        // 0 freezes every metric at its first sample; >1 makes the EWMA
        // oscillate (negative coefficient on history)
        Err("must be in (0, 1]".to_string())
    }
}

fn main() -> Result<()> {
    // Logging is opt-in via RUST_LOG (e.g. RUST_LOG=debug): it writes to
    // stderr, which would tear the raw-mode UI, so it stays off by default.
    // TODO: do something with logs so they appear in special debug pane?
    pretty_env_logger::init();
    let opt = Opt::parse();

    // ratatui::init enters the alternate screen, enables raw mode, and installs
    // a panic hook that restores the terminal. We capture the run result before
    // restoring so the terminal is always cleaned up, even on error.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &opt);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, opt: &Opt) -> Result<()> {
    let tick_rate = Duration::from_secs_f64(opt.delay);
    let mut sprocs = SProcs::default();
    let mut view = View::new(opt.delay);

    // Prime the first frame with an initial sample so the table isn't empty.
    sprocs.update(opt.ewma_weight);
    view.tick(&sprocs.get().collect::<Vec<_>>());
    let mut last_tick = Instant::now();

    loop {
        view.draw(
            terminal,
            &sprocs.summary(),
            &sprocs.get().collect::<Vec<_>>(),
        )?;

        // Block for input until it's time for the next tick. Resizes are handled
        // implicitly by the next draw, so we only need to react to keys.
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c');
                    if ctrl_c || view.handle_key(key) {
                        break;
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            sprocs.update(opt.ewma_weight);
            // per-tick view bookkeeping (flash fade, keep-alive aging) advances
            // here, not in draw -- draws also happen on every keypress
            view.tick(&sprocs.get().collect::<Vec<_>>());
            last_tick = Instant::now();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_must_be_a_positive_finite_number() {
        assert_eq!(parse_delay("0.5"), Ok(0.5));
        assert!(parse_delay("0").is_err()); // would busy-loop at 100% cpu
        assert!(parse_delay("-1").is_err()); // would panic in Duration::from_secs_f64
        assert!(parse_delay("inf").is_err());
        assert!(parse_delay("nan").is_err());
        assert!(parse_delay("fast").is_err());
    }

    #[test]
    fn ewma_weight_must_be_in_unit_interval() {
        assert_eq!(parse_ewma_weight("0.5"), Ok(0.5));
        assert_eq!(parse_ewma_weight("1"), Ok(1.0));
        assert!(parse_ewma_weight("0").is_err()); // metrics would never move
        assert!(parse_ewma_weight("1.5").is_err()); // oscillates
        assert!(parse_ewma_weight("nan").is_err());
    }
}
