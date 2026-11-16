use mdm_ipc::ErrorCode;
use rustcatch_cli::map_error_to_exit_code;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "Usage: rustcatch-cli <add|list|show|pause|resume|cancel|retry|wait|doctor|register-host>"
        );
        return ExitCode::from(2);
    }
    match args[0].as_str() {
        "doctor" => {
            println!(
                "{}",
                serde_json::json!({
                    "project": "RustCatch",
                    "protocol_version": 1,
                    "native_host": "com.rustcatch.bridge",
                    "desktop_service": "unavailable",
                    "browser_handoff": "unavailable"
                })
            );
            ExitCode::from(0)
        }
        "register-host" => desktop_service_unavailable(),
        "add" => {
            if args.len() < 4 || args[2] != "--idempotency-key" {
                eprintln!("Error: 'add' requires <url> --idempotency-key <key>");
                return ExitCode::from(map_error_to_exit_code(ErrorCode::InvalidRequest) as u8);
            }
            desktop_service_unavailable()
        }
        "list" | "show" | "pause" | "resume" | "cancel" | "retry" | "wait" => {
            desktop_service_unavailable()
        }
        _ => {
            eprintln!("Unknown subcommand: {}", args[0]);
            ExitCode::from(2)
        }
    }
}

fn desktop_service_unavailable() -> ExitCode {
    eprintln!("RustCatch CLI is not connected to a desktop download service");
    ExitCode::from(3)
}
