use crate::{HandoffRecord, TaskCoordinator, TaskRecord};
use mdm_engine::durable;
use mdm_ipc::{CURRENT_SCHEMA_VERSION, TaskState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct SavedTasks {
    schema_version: u32,
    tasks: Vec<TaskRecord>,
    sources: BTreeMap<String, String>,
    handoffs: Vec<HandoffRecord>,
    tombstones: Vec<((String, String), u64)>,
}

pub(crate) struct TaskStore {
    root: PathBuf,
    _guard: File,
}

impl TaskStore {
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        create_private_dir(root)?;
        let guard = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("desktop.lock"))?;
        guard.try_lock().map_err(io::Error::from)?;
        Ok(Self {
            root: root.to_owned(),
            _guard: guard,
        })
    }

    pub(crate) fn load(&self) -> io::Result<TaskCoordinator> {
        let file = match File::open(self.root.join("tasks.json")) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(TaskCoordinator::new()),
            Err(e) => return Err(e),
        };
        let disk: SavedTasks = serde_json::from_reader(file).map_err(io::Error::other)?;
        if disk.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "不兼容的任务存储版本",
            ));
        }
        let mut coordinator = TaskCoordinator::new();
        for mut task in disk.tasks {
            if task.schema_version != CURRENT_SCHEMA_VERSION || task.source_kind != "direct_http" {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "不兼容的下载任务记录",
                ));
            }
            task.worker_source_url = disk
                .sources
                .get(&task.task_id)
                .filter(|url| public_download(url))
                .cloned()
                .unwrap_or_default();
            if matches!(
                task.state,
                TaskState::Running
                    | TaskState::Resolving
                    | TaskState::Finishing
                    | TaskState::Pausing
            ) {
                task.state = TaskState::Paused;
                task.attempt_id = None;
                task.phase = None;
                task.revision += 1;
            } else if task.state == TaskState::Cancelling {
                task.state = TaskState::Cancelled;
                task.attempt_id = None;
                task.phase = None;
                task.revision += 1;
            }
            if !task.state.is_terminal()
                && task.worker_source_url.is_empty()
                && task.state != TaskState::HandoffPending
            {
                task.state = TaskState::AwaitingUser;
                task.reason = Some("source_reentry".into());
                task.attempt_id = None;
                task.phase = None;
            } else if !task.state.is_terminal() && task.state != TaskState::HandoffPending {
                let dir = task.output.directory_ref.as_deref().map(Path::new);
                if let Some(dir) = dir {
                    if let Ok(http) = mdm_engine::http_transfer::HttpTransfer::inspect(
                        dir,
                        &task.task_id,
                        &task.output.file_name_hint,
                    ) {
                        task.bytes_received = http.saved_bytes();
                        if let Ok(Some(_)) = http.committed_file() {
                            task.state = TaskState::Completed;
                            task.phase = Some(mdm_ipc::TaskPhase::Commit);
                            task.revision += 1;
                        }
                    }
                }
            }
            coordinator.task_by_idem.insert(
                (task.caller_key.clone(), task.idempotency_key.clone()),
                (task.task_id.clone(), task.creation_digest.clone()),
            );
            coordinator.tasks.insert(task.task_id.clone(), task);
        }
        coordinator.handoffs_by_idem = disk
            .handoffs
            .into_iter()
            .map(|record| {
                (
                    (record.caller_key.clone(), record.idempotency_key.clone()),
                    record,
                )
            })
            .collect();
        coordinator.tombstones = disk.tombstones.into_iter().collect();
        Ok(coordinator)
    }

    pub(crate) fn save(&self, coordinator: &TaskCoordinator) -> io::Result<()> {
        let mut disk = SavedTasks {
            schema_version: CURRENT_SCHEMA_VERSION,
            tasks: Vec::new(),
            sources: BTreeMap::new(),
            handoffs: coordinator.handoffs_by_idem.values().cloned().collect(),
            tombstones: coordinator
                .tombstones
                .iter()
                .map(|(key, date)| (key.clone(), *date))
                .collect(),
        };
        for task in coordinator
            .tasks
            .values()
            .filter(|task| task.source_kind == "direct_http")
        {
            let mut task = task.clone();
            if public_download(&task.worker_source_url) {
                disk.sources
                    .insert(task.task_id.clone(), task.worker_source_url.clone());
            }
            task.worker_source_url.clear();
            task.context_ref = None;
            task.source_display = non_sensitive_url(&task.source_display);
            disk.tasks.push(task);
        }
        durable::replace(
            &self.root.join("tasks.json"),
            &serde_json::to_vec(&disk).map_err(io::Error::other)?,
        )
    }
}

pub fn data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("RustCatch"));
    }
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join("Library/Application Support/RustCatch"));
    }
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".local/share"))
        })
        .map(|p| p.join("rustcatch"))
}

pub fn public_download(raw: &str) -> bool {
    raw.len() <= 4096
        && url::Url::parse(raw).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
        })
}

pub(crate) fn non_sensitive_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return String::new();
    };
    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.to_string()
}

pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    let meta = path.symlink_metadata()?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "任务目录不是普通目录",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "任务目录可被其他用户读取",
            ));
        }
    }
    Ok(())
}
