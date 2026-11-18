use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("RustCatch MCP is unavailable until it shares the desktop task service");
    ExitCode::from(3)
}
