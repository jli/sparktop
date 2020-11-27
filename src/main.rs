use std::collections::HashMap;

use anyhow::Result;
use log;
use ordered_float::OrderedFloat as OrdFloat;
use pretty_env_logger;
use structopt::StructOpt;
use sysinfo::{ProcessExt, System, SystemExt};

use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Layout};
use tui::widgets::{Block, Borders, Row, Table};
use tui::Terminal;

mod render;
mod sproc;

use sproc::SProc;

#[derive(StructOpt)]
struct Opt {
    #[structopt(short)]
    pid: Option<i32>,
    #[structopt(short)]
    num_iters: Option<usize>,
    #[structopt(short, default_value = "2.")]
    delay: f64,
    // weight given to new samples.
    #[structopt(short, default_value = "0.5")]
    ewma_weight: f64,
}

struct STerm {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl STerm {
    fn new() -> Result<Self> {
        let stdout = std::io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let terminal = tui::Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    fn draw(&mut self, sprocs: &Vec<&SProc>) -> Result<()> {
        self.terminal.clear()?;
        self.terminal.draw(|f| {
            let rects = Layout::default()
                .constraints([Constraint::Percentage(100)].as_ref())
                .margin(0)
                .split(f.size());
            let header = ["pid", "process", "CPU-e", "cpu history"];
            let rows = sprocs.iter().map(|sp| {
                let d = vec![
                    sp.pid.to_string(),
                    sp.name.clone(),
                    sp.cpu_ewma.to_string(),
                    render::render_vec(&sp.cpu_hist, 100.),
                ];
                // Learn: why doesn't .iter() work?
                Row::Data(d.into_iter())
            });
            // TODO: how to mix length and percentage?
            let tab = Table::new(header.iter(), rows)
                .block(Block::default().borders(Borders::ALL).title("Table"))
                .widths(&[Constraint::Length(6), Constraint::Length(24), Constraint::Length(5), Constraint::Percentage(99)]);
            f.render_widget(tab, rects[0]);
        })?;
        Ok(())
    }
}

fn main() -> Result<()> {
    // std::env::set_var("RUST_LOG", "debug");
    std::env::set_var("RUST_LOG", "info");
    pretty_env_logger::init();

    let opt = Opt::from_args();
    println!("hi ✨");
    let mut sys = System::new_all();
    let mut sprocs: HashMap<i32, SProc> = HashMap::new();

    let mut term = STerm::new()?;

    let mut i = 0;
    loop {
        // TODO: refresh_processes() doesn't seem to work?
        sys.refresh_all();

        // add latest data to sprocs
        let latest_procs = sys.get_processes();
        for (&pid, proc) in latest_procs {
            if let Some(pid_filter) = opt.pid {
                if pid != pid_filter {
                    continue;
                }
            }
            log::debug!("handling {} {} {}", pid, proc.name(), proc.cpu_usage());
            sprocs
                .entry(pid)
                .and_modify(|sp| sp.add_sample(proc, opt.ewma_weight))
                .or_insert(proc.into());
        }

        // clean up dead processes
        let dead_pids: Vec<i32> = sprocs
            .keys()
            .filter(|&p| !latest_procs.contains_key(p))
            .map(|&p| p)
            .collect();
        for dead_pid in dead_pids {
            log::debug!("removing dead pid: {}", dead_pid);
            sprocs.remove(&dead_pid);
        }

        // render the remainder
        let mut sprocs: Vec<_> = sprocs.values().collect();
        sprocs.sort_by_key(|sp| OrdFloat(-sp.cpu_ewma)); // negation for highest first
        term.draw(&sprocs)?;

        i += 1;
        if let Some(limit) = opt.num_iters {
            if i >= limit {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(opt.delay));
    }
    Ok(())
}
