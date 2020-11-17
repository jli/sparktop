use anyhow::{Context, Result};
use structopt::StructOpt;
use sysinfo::{ProcessExt, Process, SystemExt, System};

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

#[derive(Debug, Default)]
struct SProc {
    name: String,
    cpu: f64,
    cpu_hist: Vec<f64>,
    disk_read: Vec<u64>,
    disk_write: Vec<u64>,
}

impl From<&Process> for SProc {
    fn from(p: &Process) -> Self {
        let du = p.disk_usage();
        Self {
            name: p.name().into(),
            cpu: p.cpu_usage().into(),
            cpu_hist: vec![p.cpu_usage().into()],
            disk_read: vec![du.read_bytes],
            disk_write: vec![du.written_bytes],
        }
    }
}

impl SProc {
    fn add_sample(&mut self, p: &Process, ewma_weight: f64) {
        let du = p.disk_usage();
        let cpu64: f64 = p.cpu_usage().into();
        self.cpu = cpu64 * ewma_weight + self.cpu * (1. - ewma_weight);
        self.cpu_hist.push(p.cpu_usage().into());
        self.disk_read.push(du.read_bytes);
        self.disk_write.push(du.written_bytes);
    }

    fn render(&self) -> String {
        format!("{}\tcpu: {:.1} {:>30}", self.name, self.cpu, render_vec(&self.cpu_hist, 100.))
    }
}

fn render_vec(xs: &Vec<f64>, max: f64) -> String {
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
    if f < 0.03 { return ' ' }
    let sub_seg = 1./8.;
    let mut i = 0;
    while f > sub_seg {
        f -= sub_seg;
        i += 1;
    }
    BARS[i]
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    println!("hi ✨");
    let mut sys = System::new_all();
    if let Some(pid) = opt.pid {
        let mut sproc: Option<SProc> = None;
        for _ in 0..opt.num_iters {
            sys.refresh_all();
            let proc = sys.get_process(pid).context("get pid")?;
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
