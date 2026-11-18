//! Local, authenticated Native Host relay. Only the desktop owns the task store.

use crate::{
    TaskCoordinator, public_download, task_store::TaskStore, ui::default_download_directory,
};
use mdm_engine::durable;
use mdm_ipc::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, SyncSender},
};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const ENDPOINT: &str = "desktop-endpoint.json";
pub const NATIVE_MANIFEST: &str = "com.rustcatch.bridge.json";

#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    ticket: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    ticket: String,
    origin: String,
    instance_id: String,
    request: BrowserBridgeRequest,
}

pub(crate) struct BridgeCall {
    pub origin: String,
    pub instance_id: String,
    pub request: BrowserBridgeRequest,
    pub response: Sender<BrowserBridgeResponse>,
}

pub(crate) struct BridgeServer {
    pub pending: Receiver<BridgeCall>,
    stop: Arc<AtomicBool>,
    endpoint: PathBuf,
    thread: Option<thread::JoinHandle<()>>,
}

impl BridgeServer {
    pub(crate) fn start(root: &Path, repaint: impl Fn() + Send + 'static) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let ticket = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let config = Endpoint {
            port: listener.local_addr()?.port(),
            ticket: ticket.clone(),
        };
        let endpoint = root.join(ENDPOINT);
        durable::replace(&endpoint, &serde_json::to_vec(&config)?)?;

        let (send, pending) = mpsc::sync_channel(16);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let root = root.to_owned();
        let thread = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = accept(&mut stream, &root, &ticket, &send, &repaint);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(30))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            pending,
            stop,
            endpoint,
            thread: Some(thread),
        })
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.endpoint);
    }
}

/// Chromium IDs use 32 lowercase letters from a to p.
pub fn valid_extension_origin(origin: &str) -> bool {
    origin
        .strip_prefix("chrome-extension://")
        .and_then(|rest| rest.strip_suffix('/'))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|c| (b'a'..=b'p').contains(&c)))
}

pub fn configured_origin(root: &Path, origin: &str) -> bool {
    if !valid_extension_origin(origin) {
        return false;
    }
    let Ok(bytes) = fs::read(root.join(NATIVE_MANIFEST)) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    value["name"] == "com.rustcatch.bridge"
        && value["allowed_origins"]
            .as_array()
            .is_some_and(|origins| origins.len() == 1 && origins[0].as_str() == Some(origin))
}

/// Places the manifest in the desktop's private directory. Browser registration is separate.
pub fn save_native_manifest(root: &Path, manifest: &serde_json::Value) -> io::Result<PathBuf> {
    crate::task_store::create_private_dir(root)?;
    let path = root.join(NATIVE_MANIFEST);
    durable::replace(&path, &serde_json::to_vec_pretty(manifest)?)?;
    Ok(path)
}

/// Used by the Native Host after authenticating the extension profile on this connection.
pub fn forward(
    root: &Path,
    origin: &str,
    instance_id: &str,
    request: BrowserBridgeRequest,
) -> io::Result<BrowserBridgeResponse> {
    let endpoint: Endpoint = serde_json::from_slice(&fs::read(root.join(ENDPOINT))?)?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    configure_stream(&stream)?;
    write_frame(
        &mut stream,
        &Envelope {
            ticket: endpoint.ticket,
            origin: origin.into(),
            instance_id: instance_id.into(),
            request,
        },
    )?;
    read_frame(&mut stream)
}

fn accept(
    stream: &mut TcpStream,
    root: &Path,
    ticket: &str,
    send: &SyncSender<BridgeCall>,
    repaint: &impl Fn(),
) -> io::Result<()> {
    configure_stream(stream)?;
    let envelope: Envelope = read_frame(stream)?;
    let id = envelope.request.request_id.clone();
    if !equal_ticket(ticket, &envelope.ticket)
        || !configured_origin(root, &envelope.origin)
        || envelope.instance_id.len() != 64
        || !envelope.instance_id.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return write_frame(
            stream,
            &reject(&id, ErrorCode::PermissionDenied, "浏览器连接未获授权"),
        );
    }
    let (response, receive) = mpsc::channel();
    let call = BridgeCall {
        origin: envelope.origin,
        instance_id: envelope.instance_id,
        request: envelope.request,
        response,
    };
    if send.try_send(call).is_err() {
        return write_frame(
            stream,
            &reject(&id, ErrorCode::ToolUnavailable, "桌面下载器暂时不可用"),
        );
    }
    repaint();
    let response = receive
        .recv_timeout(Duration::from_secs(4))
        .unwrap_or_else(|_| {
            reject(
                &id,
                ErrorCode::ToolUnavailable,
                "桌面下载器响应超时，请按原标识重试",
            )
        });
    write_frame(stream, &response)
}

fn configure_stream(stream: &TcpStream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(6)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))
}

fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut prefix = [0; 4];
    reader.read_exact(&mut prefix)?;
    let len = u32::from_ne_bytes(prefix) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    let mut data = vec![0; len];
    reader.read_exact(&mut data)?;
    let value = validate_raw_frame(&data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid frame"))?;
    serde_json::from_value(value).map_err(io::Error::other)
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let data = serde_json::to_vec(value)?;
    if data.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized frame",
        ));
    }
    writer.write_all(&(data.len() as u32).to_ne_bytes())?;
    writer.write_all(&data)
}

fn equal_ticket(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0
}

fn reject(id: &str, code: ErrorCode, message: &str) -> BrowserBridgeResponse {
    BrowserBridgeResponse::error(id, code, message, false, None)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    mode: String,
    browser: BrowserMeta,
    source: SourceSpec,
    output: OutputPolicy,
    #[serde(default)]
    context_ref: Option<String>,
    #[serde(default)]
    media_intent: Option<MediaIntent>,
    network_profile_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Add {
    source: SourceSpec,
    output: OutputPolicy,
    #[serde(default)]
    context_ref: Option<String>,
    #[serde(default)]
    media_intent: Option<MediaIntent>,
    network_profile_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    task_id: String,
    expected_revision: DecimalU64,
    handoff_token: String,
    browser_download_id: u64,
    browser_cancel_confirmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Abort {
    task_id: String,
    expected_revision: DecimalU64,
    handoff_token: String,
}

fn approved_source(source: &SourceSpec, intercept: bool) -> bool {
    let SourceSpec::DirectHttp {
        url,
        page_url,
        method,
        replayability,
        replayability_evidence_ref,
    } = source
    else {
        return false;
    };
    public_download(url)
        && page_url.as_deref().is_none_or(public_download)
        && method == "GET"
        && (!intercept
            || (*replayability == Replayability::KnownReplayable
                && replayability_evidence_ref.as_deref().is_some_and(|rule| {
                    rule == format!(
                        "user-repeatable:{}",
                        hex::encode(Sha256::digest(url.as_bytes()))
                    )
                })))
}

fn browser_output(mut output: OutputPolicy) -> Option<OutputPolicy> {
    if output.directory_policy != DirectoryPolicy::CategoryDefault
        || output.directory_ref.is_some()
        || output.queue_id.is_some()
        || output.collision != CollisionPolicy::Rename
    {
        return None;
    }
    output.file_name_hint = sanitize_file_name(&output.file_name_hint).ok()?;
    output.directory_ref = Some(default_download_directory());
    Some(output)
}

pub(crate) fn dispatch(
    coordinator: &mut TaskCoordinator,
    store: &TaskStore,
    call: &BridgeCall,
    now: u64,
) -> BrowserBridgeResponse {
    let request = &call.request;
    let id = &request.request_id;
    let key = &request.idempotency_key;
    let invalid = || {
        reject(
            id,
            ErrorCode::InvalidRequest,
            "浏览器下载申请格式错误或超出授权范围",
        )
    };
    if request.version != PROTOCOL_VERSION_V1 {
        return reject(id, ErrorCode::UnsupportedVersion, "浏览器协议版本不兼容");
    }
    if id.is_empty() || id.len() > 128 || key.is_empty() || key.len() > 256 {
        return invalid();
    }
    let caller = CallerScope {
        os_user: "browser".into(),
        origin_kind: CallerOriginKind::NativeExtension,
        instance_id: call.instance_id.clone(),
        profile_id: call.origin.clone(),
        capabilities: vec![CapabilityScope::Read, CapabilityScope::Add],
    };
    let mut next = coordinator.clone();
    // Operation results are derived from the durable ledger, not old in-process ACKs.
    next.op_dedup.clear();
    let expired = next.expire_handoffs(now);
    let mut response = match request.op.as_str() {
        "handoff.prepare" => {
            let Ok(payload) = serde_json::from_value::<Prepare>(request.payload.clone()) else {
                return invalid();
            };
            if payload.mode != "browser_intercept"
                || payload.browser.installation_id != call.instance_id
                || payload.browser.profile_id != call.instance_id
                || payload.context_ref.is_some()
                || payload.media_intent.is_some()
                || payload.network_profile_id != "system-default"
                || !approved_source(&payload.source, true)
            {
                return invalid();
            }
            let Some(output) = browser_output(payload.output) else {
                return invalid();
            };
            next.prepare_handoff(
                &caller,
                id,
                key,
                &payload.browser,
                &payload.source,
                output,
                None,
                "default".into(),
                None,
                now,
            )
        }
        "handoff.commit" => {
            let Ok(p) = serde_json::from_value::<Commit>(request.payload.clone()) else {
                return invalid();
            };
            next.commit_handoff(
                &caller,
                id,
                key,
                &p.task_id,
                p.expected_revision.0,
                &p.handoff_token,
                p.browser_download_id,
                p.browser_cancel_confirmed,
                now,
            )
        }
        "handoff.abort" => {
            let Ok(p) = serde_json::from_value::<Abort>(request.payload.clone()) else {
                return invalid();
            };
            next.abort_handoff(
                &caller,
                id,
                key,
                &p.task_id,
                p.expected_revision.0,
                &p.handoff_token,
                now,
            )
        }
        "handoff.status" => {
            if !expired {
                return coordinator.query_handoff_status(&caller, id, key);
            }
            next.query_handoff_status(&caller, id, key)
        }
        "task.add" => {
            let Ok(p) = serde_json::from_value::<Add>(request.payload.clone()) else {
                return invalid();
            };
            if p.context_ref.is_some()
                || p.media_intent.is_some()
                || p.network_profile_id != "system-default"
                || !approved_source(&p.source, false)
            {
                return invalid();
            }
            let Some(output) = browser_output(p.output) else {
                return invalid();
            };
            next.add_task(
                &caller,
                id,
                key,
                &p.source,
                output,
                None,
                "default".into(),
                None,
            )
        }
        _ => return invalid(),
    };
    if !expired
        && response.status == ResponseStatus::Error
        && !response
            .error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::LeaseExpired)
    {
        return response;
    }
    if store.save(&next).is_err() {
        return reject(id, ErrorCode::InternalError, "无法持久保存浏览器交接");
    }
    // Queued jobs may be dispatched only after the disk write succeeded.
    next.op_dedup.clear();
    *coordinator = next;
    if response.status != ResponseStatus::Error {
        response.durable = Some(true);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ORIGIN: &str = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn transfer(
        root: &Path,
        server: &BridgeServer,
        coordinator: &mut TaskCoordinator,
        store: &TaskStore,
        op: &str,
        payload: serde_json::Value,
        now: u64,
    ) -> BrowserBridgeResponse {
        let request = BrowserBridgeRequest {
            version: 1,
            request_id: Uuid::new_v4().to_string(),
            idempotency_key: "download-52-fixture".into(),
            op: op.into(),
            payload,
        };
        thread::scope(|scope| {
            let client =
                scope.spawn(move || forward(root, ORIGIN, &"a".repeat(64), request).unwrap());
            let call = server.pending.recv_timeout(Duration::from_secs(3)).unwrap();
            call.response
                .send(dispatch(coordinator, store, &call, now))
                .unwrap();
            client.join().unwrap()
        })
    }

    #[test]
    fn handoff_acks_are_durable_across_desktop_restart_and_replay() {
        let scratch =
            Scratch(std::env::temp_dir().join(format!("rustcatch-handoff-{}", Uuid::new_v4())));
        let root = &scratch.0;
        save_native_manifest(
            root,
            &json!({"name": "com.rustcatch.bridge", "allowed_origins": [ORIGIN]}),
        )
        .unwrap();
        let store = TaskStore::open(root).unwrap();
        assert!(TaskStore::open(root).is_err());
        let server = BridgeServer::start(root, || {}).unwrap();
        let mut coordinator = store.load().unwrap();
        let url = "https://example.invalid/file.zip";
        let mut prepare = json!({
            "mode": "browser_intercept",
            "browser": { "installation_id": "a".repeat(64), "profile_id": "a".repeat(64), "download_id": 52,
                "tab_id": 8, "frame_id": 0, "document_id": "doc-1", "nav_epoch": 1 },
            "source": { "kind": "direct_http", "method": "GET", "url": url, "replayability": "known_replayable",
                "replayability_evidence_ref": format!("user-repeatable:{}", hex::encode(Sha256::digest(url))) },
            "output": { "directory_policy": "category_default", "file_name_hint": "file.zip" },
            "network_profile_id": "system-default"
        });
        prepare["source"]["method"] = "POST".into();
        let rejected = transfer(
            root,
            &server,
            &mut coordinator,
            &store,
            "handoff.prepare",
            prepare.clone(),
            1000,
        );
        assert_eq!(rejected.status, ResponseStatus::Error);
        assert!(coordinator.tasks.is_empty());
        prepare["source"]["method"] = "GET".into();
        let blocked = root.join("tasks.json");
        fs::create_dir(&blocked).unwrap();
        let failed_save = transfer(
            root,
            &server,
            &mut coordinator,
            &store,
            "handoff.prepare",
            prepare.clone(),
            1000,
        );
        assert_eq!(failed_save.status, ResponseStatus::Error);
        assert_ne!(failed_save.durable, Some(true));
        assert!(coordinator.tasks.is_empty());
        fs::remove_dir(blocked).unwrap();
        let pending = transfer(
            root,
            &server,
            &mut coordinator,
            &store,
            "handoff.prepare",
            prepare,
            1000,
        );
        assert_eq!(pending.status, ResponseStatus::Prepared);
        assert_eq!(pending.durable, Some(true));
        assert_eq!(pending.task_state, Some(TaskState::HandoffPending));
        let task_id = pending.task_id.unwrap();
        drop(server);
        drop(store);

        let store = TaskStore::open(root).unwrap();
        let mut coordinator = store.load().unwrap();
        assert_eq!(coordinator.tasks[&task_id].state, TaskState::HandoffPending);
        let server = BridgeServer::start(root, || {}).unwrap();
        let mut commit = json!({
            "task_id": task_id, "expected_revision": pending.revision,
            "handoff_token": pending.handoff_token, "browser_download_id": 52,
            "browser_cancel_confirmed": false
        });
        let rejected = transfer(
            root,
            &server,
            &mut coordinator,
            &store,
            "handoff.commit",
            commit.clone(),
            2000,
        );
        assert_eq!(rejected.status, ResponseStatus::Error);
        assert_eq!(coordinator.tasks[&task_id].state, TaskState::HandoffPending);
        commit["browser_cancel_confirmed"] = true.into();
        let confirmed = transfer(
            root,
            &server,
            &mut coordinator,
            &store,
            "handoff.commit",
            commit.clone(),
            3000,
        );
        assert_eq!(confirmed.status, ResponseStatus::Queued);
        assert_eq!(confirmed.durable, Some(true));
        let replay = transfer(
            root,
            &server,
            &mut coordinator,
            &store,
            "handoff.commit",
            commit,
            500_000,
        );
        assert_eq!(replay.status, ResponseStatus::Queued);
        assert_eq!(replay.task_id, confirmed.task_id);
        assert_eq!(store.load().unwrap().tasks.len(), 1);
    }
}
