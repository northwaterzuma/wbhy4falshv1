//! Native HTTP transfer with a synchronized, single-range recovery journal.

use crate::{
    durable,
    http_validator::{SourceValidator, parse_content_range},
};
use mdm_ipc::{ErrorCode, ErrorDetail, sanitize_file_name};
use reqwest::{Client, Response, StatusCode, Url, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CHECKPOINT_BYTES: u64 = 1024 * 1024;
const STATE_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
struct FileCommit {
    name: String,
    digest: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Checkpoint {
    version: u32,
    source: String,
    validator: SourceValidator,
    bytes: u64,
    #[serde(default)]
    expected_body_digest: Option<String>,
    #[serde(default)]
    commit: Option<FileCommit>,
}

pub struct HttpTransfer {
    dir: PathBuf,
    part: PathBuf,
    journal: PathBuf,
    name: String,
    checkpoint: Option<Checkpoint>,
}

fn failure(code: ErrorCode, message: &str, retryable: bool) -> ErrorDetail {
    ErrorDetail {
        code,
        message: message.into(),
        retryable,
        retry_after_ms: None,
    }
}

fn disk(error: io::Error) -> ErrorDetail {
    failure(
        ErrorCode::InternalError,
        &format!("HTTP 文件操作失败: {error}"),
        false,
    )
}

fn protocol(message: &str) -> ErrorDetail {
    failure(ErrorCode::NetworkError, message, true)
}

impl HttpTransfer {
    pub fn open(directory: &Path, task_id: &str, name: &str) -> Result<Self, ErrorDetail> {
        Self::open_inner(directory, task_id, name, true)
    }

    pub fn inspect(directory: &Path, task_id: &str, name: &str) -> Result<Self, ErrorDetail> {
        Self::open_inner(directory, task_id, name, false)
    }

    fn open_inner(
        directory: &Path,
        task_id: &str,
        name: &str,
        create: bool,
    ) -> Result<Self, ErrorDetail> {
        if task_id.is_empty()
            || task_id.len() > 80
            || !task_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(failure(ErrorCode::InvalidRequest, "无效的任务标识", false));
        }
        let name = sanitize_file_name(name)?;
        let directory = std::path::absolute(directory).map_err(disk)?;
        let dir = directory.join(format!(".rustcatch-{task_id}"));
        if create {
            fs::create_dir_all(&directory).map_err(disk)?;
            match fs::create_dir(&dir) {
                Ok(()) => durable::sync_dir(&directory).map_err(disk)?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(disk(error)),
            }
        }
        if !fs::symlink_metadata(&dir)
            .map_err(disk)?
            .file_type()
            .is_dir()
        {
            return Err(failure(ErrorCode::InvalidState, "任务暂存目录无效", false));
        }
        let journal = dir.join("http.json");
        let part = dir.join("body.part");
        let checkpoint = match regular_file(&journal)? {
            false => None,
            true => {
                let file = File::open(&journal).map_err(disk)?;
                if file.metadata().map_err(disk)?.len() > 64 * 1024 {
                    return Err(failure(ErrorCode::InvalidState, "HTTP 检查点过大", false));
                }
                let state: Checkpoint = serde_json::from_reader(file)
                    .map_err(|_| failure(ErrorCode::InvalidState, "HTTP 检查点损坏", false))?;
                if state.version != STATE_VERSION {
                    return Err(failure(
                        ErrorCode::UnsupportedVersion,
                        "HTTP 检查点版本不兼容",
                        false,
                    ));
                }
                if state
                    .validator
                    .total_bytes
                    .is_some_and(|total| state.bytes > total)
                {
                    return Err(failure(
                        ErrorCode::InvalidState,
                        "HTTP 检查点长度无效",
                        false,
                    ));
                }
                Some(state)
            }
        };
        regular_file(&part)?;
        Ok(Self {
            dir,
            part,
            journal,
            name,
            checkpoint,
        })
    }

    pub fn saved_bytes(&self) -> u64 {
        self.checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.bytes)
    }

    fn save(&self, checkpoint: &Checkpoint) -> Result<(), ErrorDetail> {
        let bytes = serde_json::to_vec(checkpoint)
            .map_err(|_| failure(ErrorCode::InternalError, "HTTP 检查点编码失败", false))?;
        durable::replace(&self.journal, &bytes).map_err(disk)
    }

    pub fn committed_file(&self) -> Result<Option<PathBuf>, ErrorDetail> {
        let Some(state) = self.checkpoint.as_ref() else {
            return Ok(None);
        };
        let Some(commit) = state.commit.as_ref() else {
            return Ok(None);
        };
        if sanitize_file_name(&commit.name)? != commit.name {
            return Err(failure(
                ErrorCode::InvalidState,
                "HTTP 提交记录的文件名无效",
                false,
            ));
        }
        let path = self
            .dir
            .parent()
            .expect("staging has parent")
            .join(&commit.name);
        if regular_file(&path)? && fingerprint(&path, state.bytes)?.as_ref() == Some(&commit.digest)
        {
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    pub async fn download(
        &mut self,
        url: &str,
        stopped: impl Fn() -> bool,
        mut progress: impl FnMut(u64),
    ) -> Result<(), ErrorDetail> {
        if self.committed_file()?.is_some() {
            return Ok(());
        }
        if let Some(state) = &self.checkpoint
            && let Some(digest) = &state.expected_body_digest
            && fingerprint(&self.part, state.bytes)?.as_ref() == Some(digest)
        {
            return Ok(());
        }
        let url = download_url(url)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let previous = attempt.previous();
                if previous.len() >= 10
                    || !anonymous_http(attempt.url())
                    || previous.last().is_some_and(|prior| {
                        prior.scheme() == "https" && attempt.url().scheme() != "https"
                    })
                {
                    attempt.error("unsupported redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| protocol("HTTP 客户端初始化失败"))?;

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&self.part)
            .map_err(disk)?;
        let mut previous = self.checkpoint.take().filter(|state| {
            state.expected_body_digest.is_none()
                && state.bytes > 0
                && state.validator.if_range_header().is_some()
                && state.validator.can_resume_reliably()
        });
        if let Some(state) = &previous
            && file.metadata().map_err(disk)?.len() < state.bytes
        {
            previous = None;
        }
        loop {
            let offset = previous.as_ref().map_or(0, |state| state.bytes);
            let mut request = client
                .get(url.clone())
                .header(header::ACCEPT_ENCODING, "identity");
            if let Some(state) = &previous {
                request = request
                    .header(header::RANGE, format!("bytes={offset}-"))
                    .header(
                        header::IF_RANGE,
                        state.validator.if_range_header().expect("strong validator"),
                    );
            }
            let response = interruptible(request.send(), &stopped).await?;
            let status = response.status();
            let length = response.content_length();
            let source = hex::encode(Sha256::digest(response.url().as_str()));
            let etag = text_header(&response, header::ETAG).filter(|tag| strong_etag(tag));
            let encoding = text_header(&response, header::CONTENT_ENCODING).unwrap_or("identity");
            if !encoding.eq_ignore_ascii_case("identity") {
                return Err(protocol("服务端返回了压缩表示，无法安全写入"));
            }
            if status == StatusCode::RANGE_NOT_SATISFIABLE {
                let old = previous
                    .as_ref()
                    .ok_or_else(|| protocol("服务端拒绝请求范围"))?;
                let range = content_range(&response)?;
                if range.start.is_some()
                    || range.end.is_some()
                    || range.total != Some(offset)
                    || old.validator.total_bytes != Some(offset)
                    || old.validator.if_range_header() != etag
                    || old.source != source
                {
                    return Err(protocol("服务端范围或文件版本已变化，保留已下载数据"));
                }
                file.set_len(offset).map_err(disk)?;
                return self.ready(file, old.clone());
            }
            let (mut state, response_bytes) = match status {
                StatusCode::OK => {
                    let state = Checkpoint {
                        version: STATE_VERSION,
                        source,
                        validator: SourceValidator::from_headers(
                            etag,
                            None,
                            length,
                            Some(encoding),
                        ),
                        bytes: 0,
                        expected_body_digest: None,
                        commit: None,
                    };
                    // Persist the reset before truncation so a crash cannot reuse the old prefix.
                    self.save(&state)?;
                    (state, length)
                }
                StatusCode::PARTIAL_CONTENT => {
                    let old = previous
                        .take()
                        .ok_or_else(|| protocol("未请求范围却收到部分响应"))?;
                    let range = content_range(&response)?;
                    let end = range.end.ok_or_else(|| protocol("缺少范围终点"))?;
                    let count = end
                        .checked_sub(offset)
                        .and_then(|n| n.checked_add(1))
                        .ok_or_else(|| protocol("HTTP 范围长度无效"))?;
                    if range.start != Some(offset)
                        || range.total != old.validator.total_bytes
                        || old.validator.if_range_header() != etag
                        || old.source != source
                        || length.is_some_and(|expected| expected != count)
                    {
                        return Err(protocol("无法核对续传范围或文件版本，保留已下载数据"));
                    }
                    (old, Some(count))
                }
                _ => return Err(status_error(status, &response)),
            };
            file.set_len(state.bytes).map_err(disk)?;
            file.seek(SeekFrom::Start(state.bytes)).map_err(disk)?;
            let stream = self
                .receive(
                    response,
                    &mut file,
                    &mut state,
                    response_bytes,
                    &stopped,
                    &mut progress,
                )
                .await;
            file.sync_all().map_err(disk)?;
            self.save(&state)?;
            stream?;
            if state
                .validator
                .total_bytes
                .is_none_or(|total| state.bytes == total)
            {
                return self.ready(file, state);
            }
            if state.bytes <= offset {
                return Err(protocol("范围请求未推进下载"));
            }
            previous = Some(state);
        }
    }

    async fn receive(
        &self,
        mut response: Response,
        file: &mut File,
        state: &mut Checkpoint,
        expected: Option<u64>,
        stopped: &impl Fn() -> bool,
        progress: &mut impl FnMut(u64),
    ) -> Result<(), ErrorDetail> {
        let start = state.bytes;
        let mut synced = start;
        progress(start);
        while let Some(chunk) = interruptible(response.chunk(), stopped).await? {
            let written = (state.bytes - start)
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| protocol("HTTP 响应长度溢出"))?;
            if expected.is_some_and(|len| written > len) {
                return Err(protocol("HTTP 内容超过声明的范围"));
            }
            file.write_all(&chunk).map_err(disk)?;
            state.bytes = start
                .checked_add(written)
                .ok_or_else(|| protocol("HTTP 文件长度溢出"))?;
            if state.bytes - synced >= CHECKPOINT_BYTES {
                file.sync_all().map_err(disk)?;
                self.save(state)?;
                synced = state.bytes;
            }
            progress(state.bytes);
        }
        if expected.is_some_and(|len| state.bytes - start != len) {
            return Err(protocol("HTTP 响应未达到声明的长度"));
        }
        Ok(())
    }

    fn ready(&mut self, file: File, mut state: Checkpoint) -> Result<(), ErrorDetail> {
        file.sync_all().map_err(disk)?;
        drop(file);
        state.expected_body_digest = fingerprint(&self.part, state.bytes)?;
        if state.expected_body_digest.is_none() {
            return Err(protocol("暂存文件与已写字节不一致"));
        }
        self.save(&state)?;
        self.checkpoint = Some(state);
        Ok(())
    }

    pub fn finish(&mut self) -> Result<PathBuf, ErrorDetail> {
        if let Some(path) = self.committed_file()? {
            return Ok(path);
        }
        let mut state = self
            .checkpoint
            .clone()
            .ok_or_else(|| failure(ErrorCode::InvalidState, "HTTP 任务尚未校验", false))?;
        let digest = state
            .expected_body_digest
            .clone()
            .ok_or_else(|| failure(ErrorCode::InvalidState, "HTTP 任务尚未校验", false))?;
        let directory = self.dir.parent().expect("staging has parent");
        for index in 0..1000 {
            let name = collision_name(&self.name, index)?;
            let target = directory.join(&name);
            if target.symlink_metadata().is_ok() {
                continue;
            }
            // State includes both size and digest before publishing, for crash reconciliation.
            state.commit = Some(FileCommit {
                name,
                digest: digest.clone(),
            });
            self.save(&state)?;
            match durable::publish_new(&self.part, &target) {
                Ok(()) => {
                    self.checkpoint = Some(state);
                    #[cfg(unix)]
                    let _ = fs::remove_file(&self.part);
                    return Ok(target);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(disk(error)),
            }
        }
        Err(failure(
            ErrorCode::InvalidState,
            "同名文件过多，请更改输出名称",
            false,
        ))
    }
}

fn regular_file(path: &Path) -> Result<bool, ErrorDetail> {
    match path.symlink_metadata() {
        Ok(m) if m.file_type().is_file() => Ok(true),
        Ok(_) => Err(failure(
            ErrorCode::InvalidState,
            "发现非普通文件，停止恢复",
            false,
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(disk(error)),
    }
}

fn fingerprint(path: &Path, len: u64) -> Result<Option<String>, ErrorDetail> {
    if !regular_file(path)? {
        return Ok(None);
    }
    let mut file = File::open(path).map_err(disk)?;
    if file.metadata().map_err(disk)?.len() != len {
        return Ok(None);
    }
    let mut sha = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(disk)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    Ok(Some(hex::encode(sha.finalize())))
}

fn collision_name(name: &str, index: usize) -> Result<String, ErrorDetail> {
    if index == 0 {
        return Ok(name.to_owned());
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    let postfix = format!(" ({index})");
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    let mut limit = stem
        .len()
        .min(240usize.saturating_sub(postfix.len() + extension.len()));
    while !stem.is_char_boundary(limit) {
        limit -= 1;
    }
    sanitize_file_name(&format!("{}{postfix}{extension}", &stem[..limit]))
}

fn anonymous_http(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

fn download_url(url: &str) -> Result<Url, ErrorDetail> {
    Url::parse(url).ok().filter(anonymous_http).ok_or_else(|| {
        failure(
            ErrorCode::InvalidRequest,
            "仅支持普通 HTTP/HTTPS 文件链接",
            false,
        )
    })
}

fn text_header(response: &Response, name: header::HeaderName) -> Option<&str> {
    response.headers().get(name)?.to_str().ok()
}

fn strong_etag(tag: &str) -> bool {
    tag.strip_prefix('"')
        .and_then(|tag| tag.strip_suffix('"'))
        .is_some_and(|value| !value.contains('"'))
}

fn content_range(
    response: &Response,
) -> Result<crate::http_validator::ContentRangeInfo, ErrorDetail> {
    text_header(response, header::CONTENT_RANGE)
        .ok_or_else(|| protocol("缺少 Content-Range"))
        .and_then(|value| parse_content_range(value).map_err(|_| protocol("Content-Range 无效")))
}

fn status_error(status: StatusCode, response: &Response) -> ErrorDetail {
    let (code, retryable) = match status {
        StatusCode::UNAUTHORIZED => (ErrorCode::AuthRequired, false),
        StatusCode::FORBIDDEN => (ErrorCode::AuthRejected, false),
        StatusCode::NOT_FOUND => (ErrorCode::NotFound, false),
        StatusCode::TOO_MANY_REQUESTS => (ErrorCode::RateLimited, true),
        _ => (ErrorCode::NetworkError, status.is_server_error()),
    };
    let mut error = failure(code, &format!("HTTP 状态 {}", status.as_u16()), retryable);
    error.retry_after_ms = text_header(response, header::RETRY_AFTER)
        .and_then(|v| v.parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000));
    error
}

async fn interruptible<T>(
    future: impl Future<Output = reqwest::Result<T>>,
    stopped: &impl Fn() -> bool,
) -> Result<T, ErrorDetail> {
    if stopped() {
        return Err(failure(ErrorCode::InvalidState, "任务已停止", false));
    }
    let stop = async {
        while !stopped() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::select! {
        biased;
        _ = stop => Err(failure(ErrorCode::InvalidState, "任务已停止", false)),
        result = future => result.map_err(|_| protocol("HTTP 连接或传输中断")),
    }
}
