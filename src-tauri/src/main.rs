// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let [_, flag, job] = args.as_slice() {
        if flag == "--write-elevated" {
            std::process::exit(cfghub::writer::run_job_file(std::path::Path::new(job)));
        }
    }
    cfghub::run();
}
