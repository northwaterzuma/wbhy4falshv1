fn main() -> std::process::ExitCode {
    eprintln!("RustCatch fault scenarios are catalogued, but the HTTP fault server is unavailable");
    std::process::ExitCode::from(3)
}
