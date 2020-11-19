use anyhow::{Context, Result};
use structopt::StructOpt;
use sysinfo::{ProcessExt, SystemExt, System};

mod render;
mod sproc;

#[derive(StructOpt)]
struct Opt {
    #[structopt(short)]
    pid: Option<i32>,
    #[structopt(short, default_value = "30")]
    num_iters: usize,
    // weight given to new samples.
    #[structopt(default_value = "0.5")]
    ewma_weight: f64,
}

// TODO:
// - use SProc for all processes
// - sort options: EWMA, latest, etc
fn main() -> Result<()> {
    let opt = Opt::from_args();
    println!("hi ✨");
    let mut sys = System::new_all();
    if let Some(pid) = opt.pid {
        let mut sproc: Option<sproc::SProc> = None;
        for _ in 0..opt.num_iters {
            sys.refresh_all();
            let proc = sys.get_process(pid).context("process not found")?;
            match sproc {
                None => sproc = Some(proc.into()),
                Some(mut sproc_inner) => {
                    sproc_inner.add_sample(proc, opt.ewma_weight);
                    sproc = Some(sproc_inner);
                }
            }
            if let Some(sproc) = &sproc {
                println!("{}", sproc.render());
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    } else {
        for _ in 0..opt.num_iters {
            sys.refresh_all();
            std::thread::sleep(std::time::Duration::from_secs(1));
            let procs = sys.get_processes();
            println!("\n=> #procs {:?}", procs.len());
            for (pid, proc) in procs {
                let cpu = proc.cpu_usage();
                if cpu > 0.01 {
                    println!("{} {} {:.2}", pid, proc.name(), cpu);
                }
            }
        }
    }
    Ok(())
}
