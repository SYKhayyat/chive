use std::process::ExitCode;

use chive::cli::run;

fn main() -> ExitCode {
    // `run` expects the full argv, since clap reads the program name from it.
    match run(std::env::args()) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("chive: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}
