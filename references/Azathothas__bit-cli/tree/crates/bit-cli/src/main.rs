//! The `bit-cli` entry point.
//!
//! Everything here does three things: build the real environment, run the
//! program, and hand the exit code to the operating system. All the logic
//! lives in the library so a test can run the same path with in-memory
//! streams and no terminal attached.

use std::io::Write;
use std::process::ExitCode;

use bit_cli::env::Env;

fn main() -> ExitCode {
    let mut env = match Env::real() {
        Ok(env) => env,
        Err(e) => {
            eprintln!("error: cannot read the process environment: {e}");
            return ExitCode::from(bit_cli_core::ExitCode::Generic.code());
        }
    };

    let code = bit_cli::run(&mut env);

    // Flush before exiting. A buffered stdout dropped at process exit can lose
    // the last write, which for a JSON document means the caller gets a
    // truncated one and a zero exit code.
    if let Err(e) = env.out.flush() {
        let _ = writeln!(env.err, "error: cannot flush stdout: {e}");
        return ExitCode::from(bit_cli_core::ExitCode::Disk.code());
    }
    let _ = env.err.flush();

    ExitCode::from(code.code())
}
