use mdm_engine::http_transfer::HttpTransfer;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const TOTAL: usize = 96 * 1024;
const INTERRUPTED: usize = 24 * 1024;
const SPLIT: usize = 56 * 1024;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustcatch-http-test-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(
    mut requests: impl FnMut(usize, String, TcpStream) + Send + 'static,
    count: usize,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/test.bin", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        for index in 0..count {
            let started = Instant::now();
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && started.elapsed() < Duration::from_secs(8) =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = String::new();
            let mut reader = BufReader::new(&stream);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                request.push_str(&line.to_ascii_lowercase());
            }
            requests(index, request, stream);
        }
    });
    (url, handle)
}

fn response(mut stream: TcpStream, status: &str, headers: String, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n"
    )
    .unwrap();
    stream.write_all(body).unwrap();
}

fn content() -> Vec<u8> {
    (0..TOTAL).map(|n| ((n * 71) % 251) as u8).collect()
}

async fn interrupted(dir: &Scratch, url: &str) -> u64 {
    let mut first = HttpTransfer::open(&dir.0, "restart-1", "sample.bin").unwrap();
    assert!(first.download(url, || false, |_| {}).await.is_err());
    drop(first);
    let second = HttpTransfer::inspect(&dir.0, "restart-1", "sample.bin").unwrap();
    assert_eq!(second.saved_bytes(), INTERRUPTED as u64);
    second.saved_bytes()
}

#[test]
fn interrupted_process_fixture() {
    let (Ok(path), Ok(url)) = (
        std::env::var("RUSTCATCH_RESTART_FIXTURE_DIR"),
        std::env::var("RUSTCATCH_RESTART_FIXTURE_URL"),
    ) else {
        return;
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut transfer =
        HttpTransfer::open(std::path::Path::new(&path), "restart-1", "sample.bin").unwrap();
    assert!(
        runtime
            .block_on(transfer.download(&url, || false, |_| {}))
            .is_err()
    );
    // Exit without running the runtime or file-owner destructors.
    std::process::exit(17);
}

#[tokio::test]
async fn reopens_checkpoint_and_resumes_short_ranges_without_overwrite() {
    let payload = content();
    let served = payload.clone();
    let (url, server) = fixture(
        move |number, request, stream| {
            if number == 0 {
                assert!(!request.contains("\nrange:"));
                response(
                    stream,
                    "200 OK",
                    format!("Content-Length: {TOTAL}\r\nETag: \"v1\"\r\n"),
                    &served[..INTERRUPTED],
                );
            } else {
                let (start, end) = if number == 1 {
                    (INTERRUPTED, SPLIT)
                } else {
                    (SPLIT, TOTAL)
                };
                assert!(request.contains(&format!("range: bytes={start}-")));
                assert!(request.contains("if-range: \"v1\""));
                response(
                    stream,
                    "206 Partial Content",
                    format!(
                        "Content-Length: {}\r\nContent-Range: bytes {}-{}/{TOTAL}\r\nETag: \"v1\"\r\n",
                        end - start,
                        start,
                        end - 1
                    ),
                    &served[start..end],
                );
            }
        },
        3,
    );
    let dir = Scratch::new();
    let exit = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "interrupted_process_fixture"])
        .env("RUSTCATCH_RESTART_FIXTURE_DIR", &dir.0)
        .env("RUSTCATCH_RESTART_FIXTURE_URL", &url)
        .output()
        .unwrap();
    assert_eq!(exit.status.code(), Some(17), "{:?}", exit.stderr);
    assert_eq!(
        HttpTransfer::inspect(&dir.0, "restart-1", "sample.bin")
            .unwrap()
            .saved_bytes(),
        INTERRUPTED as u64,
    );
    fs::write(dir.0.join("sample.bin"), b"existing user file").unwrap();
    let part = dir.0.join(".rustcatch-restart-1/body.part");
    OpenOptions::new()
        .append(true)
        .open(part)
        .unwrap()
        .write_all(b"unsynced suffix")
        .unwrap();

    let mut after_restart = HttpTransfer::open(&dir.0, "restart-1", "sample.bin").unwrap();
    after_restart
        .download(&url, || false, |_| {})
        .await
        .unwrap();
    let final_file = after_restart.finish().unwrap();
    assert_eq!(fs::read(&final_file).unwrap(), payload);
    assert_eq!(
        fs::read(dir.0.join("sample.bin")).unwrap(),
        b"existing user file"
    );
    let reopened = HttpTransfer::inspect(&dir.0, "restart-1", "sample.bin").unwrap();
    assert_eq!(reopened.committed_file().unwrap(), Some(final_file));
    server.join().unwrap();
}

#[tokio::test]
async fn ignored_range_replaces_the_prefix_instead_of_appending() {
    let replacement = vec![7u8; TOTAL];
    let new_body = replacement.clone();
    let original = content();
    let (url, server) = fixture(
        move |number, request, stream| {
            if number == 0 {
                response(
                    stream,
                    "200 OK",
                    format!("Content-Length: {TOTAL}\r\nETag: \"old\"\r\n"),
                    &original[..INTERRUPTED],
                );
            } else {
                assert!(request.contains(&format!("range: bytes={INTERRUPTED}-")));
                response(
                    stream,
                    "200 OK",
                    format!("Content-Length: {TOTAL}\r\nETag: \"new\"\r\n"),
                    &new_body,
                );
            }
        },
        2,
    );
    let dir = Scratch::new();
    interrupted(&dir, &url).await;
    let mut transfer = HttpTransfer::open(&dir.0, "restart-1", "sample.bin").unwrap();
    transfer.download(&url, || false, |_| {}).await.unwrap();
    assert_eq!(fs::read(transfer.finish().unwrap()).unwrap(), replacement);
    server.join().unwrap();
}

#[tokio::test]
async fn changed_validator_never_updates_the_part_file() {
    let original = content();
    let (url, server) = fixture(
        move |number, _, stream| {
            if number == 0 {
                response(
                    stream,
                    "200 OK",
                    format!("Content-Length: {TOTAL}\r\nETag: \"old\"\r\n"),
                    &original[..INTERRUPTED],
                );
            } else {
                response(
                    stream,
                    "206 Partial Content",
                    format!(
                        "Content-Length: {}\r\nContent-Range: bytes {INTERRUPTED}-{}/{TOTAL}\r\nETag: \"changed\"\r\n",
                        TOTAL - INTERRUPTED,
                        TOTAL - 1
                    ),
                    &original[INTERRUPTED..],
                );
            }
        },
        2,
    );
    let dir = Scratch::new();
    interrupted(&dir, &url).await;
    let mut transfer = HttpTransfer::open(&dir.0, "restart-1", "sample.bin").unwrap();
    assert!(transfer.download(&url, || false, |_| {}).await.is_err());
    assert_eq!(
        fs::read(dir.0.join(".rustcatch-restart-1/body.part")).unwrap(),
        &content()[..INTERRUPTED]
    );
    assert!(!dir.0.join("sample.bin").exists());
    server.join().unwrap();
}
