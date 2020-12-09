use anyhow::Result;
use pretty_env_logger;
use structopt::StructOpt;

mod event;
mod render;
mod sproc;
mod sprocs;
mod view;

use event::{Event, EventStream};
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
    let opt: Opt = Opt::from_args();

    let mut sprocs = SProcs::new();
    let mut view = View::new()?;
    let mut events = EventStream::new(std::time::Duration::from_secs_f64(opt.delay));
    loop {
        match events.next() {
            Event::Resize => {
                view.draw(&mut sprocs.get().collect())?;
            }
            Event::Key(k) => {
                view.handle_key(k);
                view.draw(&mut sprocs.get().collect())?;
            }
            Event::Tick => {
                sprocs.update(opt.ewma_weight);
                view.draw(&mut sprocs.get().collect())?;
            }
        }
    }
    // Note: no Ok because it's unreachable.
}
