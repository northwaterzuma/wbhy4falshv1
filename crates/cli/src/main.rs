use mdm_ipc::ErrorCode;
use rustcatch_cli::{generate_native_host_manifest, map_error_to_exit_code};
use std::path::Path;
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
                    "browser_handoff": "requires_native_manifest_and_running_desktop"
                })
            );
            ExitCode::from(0)
        }
        "register-host" => {
            let [_, exe, id] = args.as_slice() else {
                eprintln!(
                    "Usage: rustcatch-cli register-host <absolute-native-host-path> <extension-id>"
                );
                return ExitCode::from(2);
            };
            let origin = format!("chrome-extension://{id}/");
            if !Path::new(exe).is_absolute()
                || !Path::new(exe).is_file()
                || !mdm_app::browser_bridge::valid_extension_origin(&origin)
            {
                eprintln!("The native host path or Chromium extension ID is invalid");
                return ExitCode::from(2);
            }
            let Some(root) = mdm_app::data_dir() else {
                return desktop_service_unavailable();
            };
            let manifest = generate_native_host_manifest(exe, std::slice::from_ref(id));
            match mdm_app::browser_bridge::save_native_manifest(&root, &manifest) {
                Ok(path) => {
                    println!("{}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Could not save the Native Messaging manifest: {error}");
                    ExitCode::from(3)
                }
            }
        }
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
