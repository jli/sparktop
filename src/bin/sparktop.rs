use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use structopt::StructOpt;

use sparktop::{sprocs::SProcs, view::View};

#[derive(StructOpt)]
struct Opt {
    #[structopt(short, default_value = "1.")]
    delay: f64,
    // weight given to new samples.
    #[structopt(short, default_value = "0.5")]
    ewma_weight: f64,
}

fn main() -> Result<()> {
    // TODO: do something with logs so they appear in special debug pane?
    std::env::set_var("RUST_LOG", "info");
    pretty_env_logger::init();
    let opt = Opt::from_args();

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
    let mut last_tick = Instant::now();

    loop {
        view.draw(terminal, &mut sprocs.get().collect::<Vec<_>>())?;

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
            last_tick = Instant::now();
        }
    }
    Ok(())
}
