use anyhow::Result;
use sysinfo::{ProcessExt, SystemExt, System};

fn main() -> Result<()> {
    println!("hi ✨");
    let mut sys = System::new_all();
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        sys.refresh_all();
        let procs = sys.get_processes();
        println!("\n=> #procs {:?}", procs.len());
        for (pid, proc) in procs {
            let cpu = proc.cpu_usage();
            if cpu > 0.01 {
                println!("{} {} {:.2}", pid, proc.name(), cpu);
            }
        }
    }
    Ok(())
}
