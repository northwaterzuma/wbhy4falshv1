use mdm_ipc::{BrowserBridgeRequest, BrowserBridgeResponse, ErrorCode};
use rustcatch_nmhost::NativeHostSession;
use std::io;

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    while let Some(frame_res) = NativeHostSession::read_native_frame(&mut reader)? {
        let resp = match frame_res {
            Ok(bytes) => service_unavailable(&bytes),
            Err(err) => {
                BrowserBridgeResponse::error("frame-err", err.code, err.message, false, None)
            }
        };
        NativeHostSession::write_native_frame(&mut writer, &resp)?;
    }
    Ok(())
}

// A standalone session cannot durably accept or execute a browser download.
fn service_unavailable(bytes: &[u8]) -> BrowserBridgeResponse {
    let request_id = serde_json::from_slice::<BrowserBridgeRequest>(bytes)
        .ok()
        .map(|request| request.request_id)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .unwrap_or_else(|| "bridge-unavailable".to_string());
    BrowserBridgeResponse::error(
        request_id,
        ErrorCode::ToolUnavailable,
        "No durable RustCatch desktop handoff service is available",
        false,
        None,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn standalone_host_never_acknowledges_handoff() {
        let response =
            super::service_unavailable(include_bytes!("../../../examples/capture_prepare.json"));
        assert_eq!(response.request_id, "req-demo-prepare");
        assert!(response.durable.is_none());
        assert_eq!(
            response.error.unwrap().code,
            super::ErrorCode::ToolUnavailable
        );
    }
}
