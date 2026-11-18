use mdm_app::{
    AppAction, RustCatchDesktopApp, TaskRecord, UiDialog, WorkerEvent, default_download_directory,
    task_matches_category,
};
use mdm_ipc::{ControlOp, ErrorCode, ErrorDetail, ResponseStatus, TaskState};

#[test]
fn desktop_reloads_http_tasks_without_persisting_a_signed_query() {
    let root = std::env::temp_dir().join(format!("rustcatch-ui-{}", uuid::Uuid::new_v4()));
    let public = "https://example.invalid/file.zip";
    let signed = "https://example.invalid/signed.zip?signature=fixture-secret";
    {
        let mut app = RustCatchDesktopApp::open(&root).unwrap();
        for url in [public, signed] {
            assert!(app.prepare_add_url(url));
            assert!(app.pending_direct_http);
            app.pending_start_now = false;
            assert!(app.confirm_pending_download());
        }
    }
    let saved = std::fs::read_to_string(root.join("tasks.json")).unwrap();
    assert!(saved.contains(public));
    assert!(!saved.contains("fixture-secret"));
    {
        let app = RustCatchDesktopApp::open(&root).unwrap();
        assert_eq!(app.coordinator.tasks.len(), 2);
        let public = app.coordinator.tasks.values().find(|task| task.source_display.ends_with("/file.zip")).unwrap();
        assert_eq!(public.state, TaskState::Queued);
        assert_eq!(public.worker_source_url, "https://example.invalid/file.zip");
        let signed = app.coordinator.tasks.values().find(|task| task.source_display.ends_with("/signed.zip")).unwrap();
        assert_eq!(signed.state, TaskState::AwaitingUser);
        assert_eq!(signed.reason.as_deref(), Some("source_reentry"));
        assert!(signed.worker_source_url.is_empty());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pausing_waits_for_worker_exit_and_fences_old_completion() {
    let mut app = RustCatchDesktopApp::default();
    assert!(app.prepare_add_url("https://example.invalid/watch?token=synthetic"));
    assert!(!app.status_message.contains("synthetic"));
    app.pending_start_now = false;
    assert!(app.confirm_pending_download());
    let id = app.coordinator.tasks.keys().next().unwrap().clone();
    let caller = RustCatchDesktopApp::ui_caller();
    for (request_id, op) in [("start", ControlOp::Resume), ("pause", ControlOp::Pause)] {
        let revision = app.coordinator.tasks[&id].revision;
        let reply = app
            .coordinator
            .control_task(&caller, request_id, &id, revision, op, 0);
        assert_eq!(reply.status, ResponseStatus::Ok);
    }
    let attempt_id = app.coordinator.tasks[&id].attempt_id.clone().unwrap();
    assert_eq!(app.coordinator.tasks[&id].state, TaskState::Pausing);
    app.record_worker_event(WorkerEvent::Completed {
        task_id: id.clone(),
        attempt_id: attempt_id.clone(),
    });
    assert_eq!(app.coordinator.tasks[&id].state, TaskState::Pausing);
    app.record_worker_event(WorkerEvent::Stopped {
        task_id: id.clone(),
        attempt_id,
    });
    assert_eq!(app.coordinator.tasks[&id].state, TaskState::Paused);
    assert!(app.coordinator.tasks[&id].attempt_id.is_none());
    for (request_id, op) in [("resume", ControlOp::Resume), ("cancel", ControlOp::Cancel)] {
        let revision = app.coordinator.tasks[&id].revision;
        let reply = app
            .coordinator
            .control_task(&caller, request_id, &id, revision, op, 0);
        assert_eq!(reply.status, ResponseStatus::Ok);
    }
    assert_eq!(app.coordinator.tasks[&id].state, TaskState::Cancelling);
    let attempt_id = app.coordinator.tasks[&id].attempt_id.clone().unwrap();
    app.record_worker_event(WorkerEvent::Stopped {
        task_id: id.clone(),
        attempt_id,
    });
    assert_eq!(app.coordinator.tasks[&id].state, TaskState::Cancelled);
}

#[test]
fn add_url_prepares_download_info_for_youtube_and_creates_task() {
    let mut app = RustCatchDesktopApp::default();

    assert!(app.prepare_add_url("https://youtu.be/l7sCL3geF0U"));
    assert!(matches!(app.open_dialog, Some(UiDialog::DownloadInfo)));
    assert_eq!(app.pending_media_id.as_deref(), Some("l7sCL3geF0U"));
    assert_eq!(app.pending_file_name, "youtube_l7sCL3geF0U.mp4");

    app.pending_start_now = false;
    assert!(app.confirm_pending_download());
    assert_eq!(app.coordinator.tasks.len(), 1);

    let task = app
        .coordinator
        .tasks
        .values()
        .next()
        .expect("task must exist");
    assert_eq!(task.source_kind, "page_url");
    assert_eq!(task.state, TaskState::Queued);
    assert!(task.media_intent.is_some());
    assert_eq!(
        app.add_url_history,
        vec!["https://youtu.be/l7sCL3geF0U".to_string()]
    );
    assert!(app.open_dialog.is_none());
}

#[test]
fn empty_url_reports_error_and_does_not_create_task() {
    let mut app = RustCatchDesktopApp::default();

    assert!(!app.prepare_add_url("   "));
    assert!(app.dialog_error.is_some());
    assert!(app.coordinator.tasks.is_empty());
}

#[test]
fn prepare_add_url_action_skips_the_extra_click_when_address_is_present() {
    let mut app = RustCatchDesktopApp::default();

    app.apply_action(AppAction::PrepareAddUrl(
        "https://youtu.be/l7sCL3geF0U".to_string(),
    ));

    assert!(matches!(app.open_dialog, Some(UiDialog::DownloadInfo)));
    assert_eq!(app.add_url_input, "https://youtu.be/l7sCL3geF0U");
}

#[test]
fn worker_failure_marks_task_failed_and_writes_log() {
    let mut app = RustCatchDesktopApp::default();
    assert!(app.prepare_add_url("https://youtu.be/l7sCL3geF0U"));
    assert!(app.confirm_pending_download());

    let (task_id, attempt_id) = app
        .coordinator
        .tasks
        .values()
        .next()
        .map(|task| {
            (
                task.task_id.clone(),
                task.attempt_id.clone().expect("attempt id"),
            )
        })
        .expect("task must exist");

    app.record_worker_event(WorkerEvent::Failed {
        task_id: task_id.clone(),
        attempt_id,
        error: ErrorDetail {
            code: ErrorCode::ToolUnavailable,
            message: "组件未解析".to_string(),
            retryable: false,
            retry_after_ms: None,
        },
    });

    let task = app
        .coordinator
        .tasks
        .get(&task_id)
        .expect("task must exist");
    assert_eq!(task.state, TaskState::Failed);
    assert_eq!(task.error_code.as_deref(), Some("ToolUnavailable"));
    assert!(
        app.log_lines
            .iter()
            .any(|line| line.message.contains("组件未解析"))
    );
}

#[test]
fn default_directory_is_downloads_and_prefills_download_info() {
    let dir = default_download_directory();
    assert!(
        dir.ends_with("Downloads") || dir == "Downloads",
        "default directory should be a Downloads folder, got {}",
        dir
    );

    let mut app = RustCatchDesktopApp::default();
    assert!(app.prepare_add_url("https://youtu.be/l7sCL3geF0U"));
    assert_eq!(app.pending_directory, dir);

    app.pending_start_now = false;
    assert!(app.confirm_pending_download());
    let task = app.coordinator.tasks.values().next().expect("task exists");
    assert_eq!(task.output.directory_ref.as_deref(), Some(dir.as_str()));
}

#[test]
fn category_filter_splits_finished_and_unfinished_tasks() {
    let mut app = RustCatchDesktopApp::default();
    assert!(app.prepare_add_url("https://youtu.be/l7sCL3geF0U"));
    app.pending_start_now = false;
    assert!(app.confirm_pending_download());
    let id = app.coordinator.tasks.keys().next().cloned().expect("task id");

    let all: Vec<&TaskRecord> = app.coordinator.tasks.values().collect();
    assert_eq!(all.iter().filter(|t| task_matches_category(t, "所有下载 (All Downloads)")).count(), 1);
    assert_eq!(all.iter().filter(|t| task_matches_category(t, "未完成 (Unfinished)")).count(), 1);
    assert_eq!(all.iter().filter(|t| task_matches_category(t, "已完成 (Finished)")).count(), 0);
    assert_eq!(all.iter().filter(|t| task_matches_category(t, "视频 (Video)")).count(), 1);
    assert_eq!(all.iter().filter(|t| task_matches_category(t, "主队列 (Main Queue)")).count(), 1);

    if let Some(task) = app.coordinator.tasks.get_mut(&id) {
        task.state = TaskState::Completed;
    }
    let all: Vec<&TaskRecord> = app.coordinator.tasks.values().collect();
    assert_eq!(all.iter().filter(|t| task_matches_category(t, "已完成 (Finished)")).count(), 1);
    assert_eq!(all.iter().filter(|t| task_matches_category(t, "未完成 (Unfinished)")).count(), 0);
}

#[test]
fn menu_actions_open_visible_dialogs() {
    let mut app = RustCatchDesktopApp::default();

    app.apply_action(AppAction::OpenAddUrl);
    assert!(matches!(app.open_dialog, Some(UiDialog::AddUrl)));

    app.apply_action(AppAction::OpenOptions);
    assert!(matches!(app.open_dialog, Some(UiDialog::Options)));

    app.apply_action(AppAction::OpenAbout);
    assert!(matches!(app.open_dialog, Some(UiDialog::About)));

    app.apply_action(AppAction::OpenTaskLog);
    assert!(matches!(app.open_dialog, Some(UiDialog::TaskLog)));

    app.apply_action(AppAction::CloseDialog);
    assert!(app.open_dialog.is_none());
}
