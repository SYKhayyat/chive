use std::process::ExitCode;

use chive::cli::run;

/// Return SIGPIPE to its default behaviour, so writing to a closed pipe
/// (`chive ... | head`) terminates quietly instead of panicking in the std
/// printer. This is the usual, correct convention for Unix CLI tools.
///
/// It lives here rather than in `cli::run` because it is process-global state:
/// a library that installs a signal handler mutates its host on the way past.
fn restore_sigpipe() {
    #[cfg(unix)]
    // SAFETY: `signal` with SIG_DFL restores the disposition this process
    // inherited; it is async-signal-safe and touches no memory chive owns.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> ExitCode {
    restore_sigpipe();
    // `run` expects the full argv, since clap reads the program name from it.
    match run(std::env::args()) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("chive: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}
