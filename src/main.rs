use anyhow::Result;
use structopt::StructOpt;

mod event;
mod render;
mod sproc;
mod sprocs;
mod view;

use event::{Event, EventStream, Next};
use sprocs::SProcs;
use view::View;

#[derive(StructOpt)]
struct Opt {
    #[structopt(short, default_value = "1.")]
    delay: f64,
    // weight given to new samples.
    #[structopt(short, default_value = "0.5")]
    ewma_weight: f64,
}

fn main() -> Result<()> {
    // std::env::set_var("RUST_LOG", "debug");
    std::env::set_var("RUST_LOG", "info");
    pretty_env_logger::init();
    let opt = Opt::from_args();

    let mut sprocs = SProcs::new();
    let mut view = View::new()?;
    let mut events = EventStream::new(std::time::Duration::from_secs_f64(opt.delay));
    loop {
        let mut next = Next::Continue;
        match events.next() {
            Event::Resize => {
                view.draw(&mut sprocs.get().collect())?;
            }
            Event::Key(k) => {
                next = view.handle_key(k);
                view.draw(&mut sprocs.get().collect())?;
            }
            Event::Tick => {
                sprocs.update(opt.ewma_weight);
                view.draw(&mut sprocs.get().collect())?;
            }
        }
        if next == Next::Quit {
            break;
        }
    }
    Ok(())
}
