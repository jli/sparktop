use anyhow::Result;
use structopt::StructOpt;

use sparktop::{
    event::{Event, EventStream, Next},
    sprocs::SProcs,
    view::View,
};

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
    // std::env::set_var("RUST_LOG", "debug");
    pretty_env_logger::init();
    let opt = Opt::from_args();

    let mut sprocs = SProcs::new();
    let mut view = View::new()?;
    let mut events = EventStream::new(std::time::Duration::from_secs_f64(opt.delay));
    loop {
        let next = match events.next() {
            Event::Resize => Next::Continue,
            Event::Key(k) => view.handle_key(k),
            Event::Tick => {
                sprocs.update(opt.ewma_weight);
                Next::Continue
            }
        };
        match next {
            Next::Continue => view.draw(&mut sprocs.get().collect())?,
            Next::Quit => break,
        }
    }
    Ok(())
}
