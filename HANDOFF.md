# RustCatch 开发交接

## 普通 HTTP 下载、重启续传和浏览器接管（2026-11-17）

- [x] 在当前 WorkBuddy 对应的受管工作树应用上一轮修复。工作树的基线 HEAD 为 `cd02a9d`，`5e5d4f2` 及本轮实现为工作区改动。上一轮 GitHub `fc5b98e` 的改动已核对一致，CI 运行号为 `44206545642`。
- [x] 普通文件由 Rust 原生 HTTP/HTTPS 单流传输。按强 ETag、最终资源身份、长度和 Range 恢复已同步字节。完整 200 从零写入，避免拼接不同版本。任务、检查点与待提交成品记入独立日志。带查询参数的地址不入普通任务记录，重启后须重新输入。
- [x] Chromium 扩展接入真实 Native Host 与桌面任务账本。严格匹配用户授权公开 GET，先保存 prepare，再核实浏览器取消，最后持久化 commit。失去反馈时，以原任务重试或在桌面明确核对。
- [x] Windows GNU / Rust 1.95.0 的 `cargo test --workspace --offline --locked` 为 81 项通过、0 失败。`cargo build --workspace --offline --locked` 通过。受控本机服务覆盖断流、进程退出后续传、短 206、源变更、200 重写、同名保护。真实回环传输覆盖持久化失败不回 ACK、重建桌面后继续核对、commit 幂等。
- [x] `node --test tests/browser_handoff.test.mjs` 两项通过，扩展脚本语法和 Python 夹具检查通过。用构建出的 CLI 与 Native Host 在临时数据目录验证清单生成、二进制帧握手、来源拒绝及桌面离线回退。
- [x] 本轮源码已提交到交付仓库并通过 GitHub Actions 三平台验证：分支 `rustcatch/http-resume-handoff-20261117`，提交 `d8211db`，CI run `44213586615`，4/4 jobs 全部成功。Windows / Ubuntu / macOS 的 Rust 单元测试与集成测试各为 81 项通过、0 失败；规格与夹具完整性 job 通过，其中包含浏览器接管的无浏览器顺序测试。CI 链接：https://github.com/northwaterzuma/wbhy4falshv1/actions/runs/44213586615
- [x] 已打标签 `v0.1.6-http-resume-handoff` 并发布预发布 Release：发布流水线 run `44215672193` success（4m7s），`Resolve and Verify Media Components` 与 `Publish to GitHub Release` 均通过；资产 `rustcatch-win64-portable-unsigned-test.zip`，204,776,320 bytes，GitHub digest `sha256:0c59860cfad0f333aa7b1e558dd422d52b330186fcd7eea2be1d4fbc7137120a`。详见 [发布证据](evidence/RELEASE-v0.1.6-http-resume/RESULTS.md)。
- [ ] 真实 Chrome/Edge 安装接管和桌面窗口测试仍为 needs_env。注册说明见 [浏览器下载接管](docs/EXTENSION_INSTALL.md)。多连接调度、带身份的接管、实际掉电恢复及媒体跨重启仍按原规格验收。

## 上一轮修复分支（2026-11-17）

- 分支 `codex/rustcatch-repair`，基于用户指定 WorkBuddy 仓库的 `cd02a9d`。在独立本地副本中实施，原开发分支保留。
- 桌面媒体任务在自己的目录暂存，按 yt-dlp 报告校验成品及必要音视频轨后提交，同名文件保留。暂停和取消等待工作进程结束。普通日志使用阶段和错误分类，未接入的网络与授权配置明确报错。
- Native Host 在持久化任务服务可用前拒绝接管，CLI/MCP 的未接入命令返回不可用。测试脚本只检查实际读取的资料与夹具。根目录规格、任务状态、首发与全量对齐评审已按当前证据修订。
- Windows GNU 的 Rust 1.95.0 执行 `cargo test --workspace --offline --locked`，74 项通过。覆盖本地父子进程停止、停止与提交互斥、同名文件、无效成品、迟到事件、宿主拒绝接管。CLI 的合成添加请求返回退出码 3。14 份 JSON、3 个二进制夹具、6 个媒体清单路径及 4 个扩展脚本语法检查通过。
- GitHub 三平台 CI 与真实媒体组件联调待运行。按用户安排，由用户把此分支推到 `northwaterzuma/wbhy4falshv1` 的新 `rustcatch/*` 分支触发 CI。后续首要任务是接入原生 HTTP 和持久化任务恢复，然后联调浏览器接管、CLI/MCP 以及代理配置。

## 历史交接与发布记录

下方历史 CI 仅适用于当时的提交。以模拟结果写入的历史验收条目以 [项目交付评审](docs/PROJECT_COMPLETION_REVIEW.md) 的证据边界为准。

```yaml
project: RustCatch
spec_version: "1.0"
updated_at_utc: "2026-11-16T13:33:20Z"
task_id: P3-T02 / P4-T02 (verified component release)
status: component_release_built_online_download_needs_user_acceptance
repo_path: https://github.com/northwaterzuma/rustcatch (branch rustcatch/dev-v1; local clone C:/Users/admin/WorkBuddy/2026-11-16-01-42-58/rustcatch)
branch: rustcatch/dev-v1
commit: 96f48dcaf637a553af270c27bfb2626a3ca38e19
upstream_base: 605a0c74e50361afe77bf1721d7172636bd5c57e
worktree_status: clean at commit
changed_files:
  - crates/ipc/src/validation.rs
  - crates/engine/src/planner.rs
  - crates/engine/src/zip_preview.rs
  - crates/tools/src/streams.rs
  - crates/app/src/grabber.rs
  - crates/engine/tests/product_logic.rs
  - crates/tools/tests/media_logic.rs
  - crates/app/tests/scheduler_logic.rs
  - Cargo.lock
requirements: [REQ-02, REQ-03, REQ-06, REQ-07, REQ-08, REQ-11]
tests_run:
  - "GitHub Actions run 43856090950 / commit 56d638b: ALL 4 jobs SUCCESS (latest verified, includes CJK font fix)"
  - "GitHub Actions run 43848343635 / commit fc120a7: ALL 4 jobs SUCCESS"
  - "cargo test --workspace on windows-latest (msvc), ubuntu-latest, macos-latest: 54 passed, 0 failed on each"
  - "cargo build --release --workspace on all 3 targets: SUCCESS"
  - "cargo +stable check --workspace --all-targets local: PASS"
  - "Python offline contract suite: PASS (schema/protocol simulation only, NOT Rust product tests)"
tests_not_run:
  - "cargo test --workspace locally: BLOCKED — no C linker (dlltool CreateProcess fails in sandbox); runs on GitHub CI windows/msvc + ubuntu + macos instead"
  - "TEST-01A/B/C: needs_env (Windows 10/11 GUI VM + IDM baseline)"
  - "TEST-04C/05C: needs_env (real Edge/Chrome browser VM)"
  - "TEST-06A/B/C/D + 07A: needs_env (authorized YouTube samples)"
  - "TEST-11B/13A/B/14A/18A/19A: needs_env (installer VM, controlled FTP/TLS/AD servers, macOS/Linux VMs)"
evidence_paths:
  - evidence/CI-fc120a7/RESULTS.md
  - evidence/CI-3066a4e/RESULTS.md
  - evidence/CI-40167f3/RESULTS.md
  - evidence/RELEASE-v0.1.3-components/RESULTS.md
  - .github/workflows/ci.yml
  - evidence/P0/ ... evidence/P5-T04/ (prior-session artifacts, unverified claims)
  - crates/engine/tests/product_logic.rs
  - crates/tools/tests/media_logic.rs
  - crates/app/tests/scheduler_logic.rs
decisions:
  - "Prior session's 7 commits (P0-P5) are preserved, not overwritten. Remote history unrelated to upstream MDM; from-scratch rewrite (5213 LOC Rust, 43 files)."
  - "Prior HANDOFF claimed '26 tests passed'; corrected: those were Python protocol simulations. Only 4 Rust tests existed, one of which was broken."
  - "All compilation runs on GitHub Actions as required. Local machine has Rust 1.98/1.95 (windows-gnu) but no working C linker."
  - "Found and fixed real source corruption from prior file writes: literal CR/LF in byte literals, unescaped quotes, Rust-2024 reserved keyword 'gen'."
  - "Found and fixed a real CI test bug: DecimalU64 test passed bare JSON integer to a string-only deserializer."
  - "Added 31 genuine Rust tests (35 total) calling actual product functions for HTTP Range, planner fencing, checkpoint crash recovery, yt-dlp error classification, format selection, scheduler dedup."
blockers:
  - "Local cargo test cannot link (no gcc/dlltool); GitHub CI is authoritative."
  - "No Windows GUI VM / browser VM / authorized YouTube sample / code-signing credential → corresponding acceptance gates remain needs_env."
next_action: "v0.1.3-components release contains resolved_verified components and passed 60 Rust tests on three platforms. Next: user accepts Windows GUI run with https://youtu.be/l7sCL3geF0U; only then mark online YouTube download as verified; continue remaining P5 needs_env items."
```

## 最近完成（真实）

1. **读文档**：START_HERE.md、AGENTS.md、RUST_DOWNLOAD_MANAGER_SPEC.md（1291 行）、TASKS.md、SOURCE_LOCK.json、ACCEPTANCE.md 已读。
2. **工作区检查**：本地工作区原为空；GitHub 仓库 `northwaterzuma/rustcatch` 已有 7 个 P0–P5 提交，全部保留。上游基线 `605a0c7` 已单独克隆并保留为 `upstream/v0.5.2` ref。
3. **复现构建并修复真实编译失败**：GitHub CI 原始运行 (43842660189) 三平台全部编译失败，根因是 prior 会话写文件时丢失转义字符：
   - `crates/ipc/src/validation.rs`: 字节字面量含裸 CR/LF、测试字符串双引号
   - `crates/engine/src/planner.rs`: `gen` 在 Rust 2024 是保留关键字
   - `crates/engine/src/zip_preview.rs`: 反斜杠字符字面量未转义
   - `crates/tools/src/streams.rs` / `crates/app/src/grabber.rs`: `\"` 被吞导致字符串字面量破碎
   修复后 `cargo +stable check --workspace --all-targets` 本地通过。
4. **修复真实测试失败**：CI 运行 43844382676 显示 Windows/macOS/Ubuntu 均因 `test_decimal_u64_bounds` 失败（测试把裸整数传给只接受字符串的 DecimalU64 反序列化器）。已修正测试为带引号 JSON 字符串。
5. **新增真实 Rust 测试**：从 4 个增至 35 个，全部调用真实产品函数，而非 Python 重实现。

## 证据边界与未运行项

- Python `tests/run_offline_verification.py` 是协议/种子文件的模拟校验，不调用 Rust 产品代码；不能作为产品功能通过的证据。
- 本地无法链接（沙箱无 gcc；Rust 自带 dlltool 在此环境 CreateProcess 失败）；真实编译与测试全部交由 GitHub Actions。
- 所有 needs_env 门槛（GUI 真机、浏览器、授权在线 YouTube、FTP/TLS/AD 受控域、安装签名）按规格保留，不得写成通过。

## 接手者立即执行

1. 核对 `git status` 与上方 commit；查看 GitHub Actions run 43848343635（commit fc120a7）的三个 Rust 矩阵 job 结论，证据见 `evidence/CI-fc120a7/RESULTS.md`。
2. 若失败，读真实日志修根因；若通过，把 job ID 与通过时间写入证据目录并更新 TASKS.md。
3. 继续 P5 时坚持同一标准：真实产品函数 + 真实测试 + 真实证据，缺环境标 needs_env。


## 构建门槛：已通过（2026-11-16）

GitHub Actions run 43845781548（commit 40167f3）全部 4 个 job success：
- windows-latest (msvc) / ubuntu-latest / macos-latest 三个目标均完成 cargo check、cargo test、cargo build --release
- Rust 测试 **35 passed / 0 failed** 每个平台（ipc 4 + engine product_logic 20 + tools media_logic 8 + app scheduler_logic 3）
- 证据：`evidence/CI-40167f3/RESULTS.md`，含 job ID 与历史失败根因

这验证了"能编译、能测试、能出 release 二进制"，**不等于**产品功能验收通过。GUI 真机、浏览器接管、授权在线 YouTube、安装签名、FTPS/AD 受控域、跨平台桌面行为均保持 needs_env。


## P2 桥接实现 + 测试（2026-11-16）

- 发现并修复真实功能缺口：`NativeHostSession::handle_raw_frame` 原先只分发 capture.publish/context.put；已补全 `handoff.prepare/commit/status/abort`、`task.add/get/control` 到唯一 TaskCoordinator，使用最小权限 CallerScope（扩展无 DeleteFiles 权限），并把 profile_id 绑定到认证会话防跨 profile 冒用。
- 新增 10 个端到端 Rust 测试（crates/nmhost/tests/handoff_protocol.rs），GitHub CI 三平台全部通过。
- 真实测试总数 45 passed / 0 failed（见 evidence/CI-3066a4e/RESULTS.md）。
- 这仍非浏览器真机验收；TEST-04C/05C 保持 needs_env。

## 最新构建门槛（覆盖上方历史数字）

GitHub Actions run 43856090950（commit 56d638b）全部 4 个 job success，三平台 cargo check / test /
build --release 均完成。证据：GitHub Actions run 页面。上一个稳定基线为 run 43848343635（fc120a7），
单平台 54 passed / 0 failed，见 `evidence/CI-fc120a7/RESULTS.md`。上方出现的 35 / 45 为历史通过数。

## 中文乱码修复 + v0.1.0 重新发布（2026-11-16）

- 根因：egui 0.29 默认字体（ProggyClean/Hack）不含 CJK 字形，菜单/状态栏中文渲染成豆腐块/乱码。
- 修复：新增 `crates/app/src/fonts.rs`，用 `include_bytes!` 内嵌 Noto Sans SC（OFL-1.1）GB2312 常用字子集
  （`assets/fonts/NotoSansSC-subset.ttf`，2.1MB，静态 wght=400），注册为 Proportional/Monospace 首选字体，
  在 `main.rs` 的 eframe 启动回调里调用 `install_chinese_font`。许可证见 `assets/fonts/OFL.txt`，
  归属已写入 `THIRD_PARTY_NOTICES` 第 6 节。
- 发布：删除 v0.1.0 旧 zip 资产 → 将 tag 从 e0a3b37 强推到 56d638b → 重跑 release 工作流
  （run 43857210839 success，重复的 dispatch run 43857233125 已取消）。
- 新包：`rustcatch-win64-portable-unsigned-test.zip`，4.5MB，SHA-256 `684d3ce7…e11ff0`，
  含 4 个 exe + 扩展包。已下载解包验证 `rustcatch.exe` 中确实内嵌字体字节（偏移 2547696）。
- 仍需真机目视确认菜单中文显示正常（本机为离线 Windows 无 C linker，无法本地启动 GUI）。


## 仓库迁移与最终验证（2026-11-16）

- 按用户指令新建公开仓库 `northwaterzuma/wbhy4falshv1`：https://github.com/northwaterzuma/wbhy4falshv1
- 完整项目（含全部历史、文档、源码、证据、CI/Release 工作流）已推送到该仓库的 `main` 与 `rustcatch/dev-v1` 两个分支，均指向 `da2a7e0ed1c71f9ea0a4f0371a55723d146f45cf`。
- 新仓库 CI run 43859735381：4/4 jobs success（契约套件 + Windows/MSVC、Ubuntu、macOS 三平台 cargo check/test/build --release）。
- 源码侧最新测试实测：`cargo test --workspace` 三平台均 **54 passed / 0 failed**（rustcatch 仓库 run 43858104281，job 133267612430）。
- 保留原 `northwaterzuma/rustcatch` 仓库不删除；wbhy4falshv1 为本次要求的交付仓库。
- needs_env 门槛不变：GUI 真机、真实浏览器接管、授权在线 YouTube、安装签名、受控 FTPS/AD、macOS/Linux 桌面行为仍未验证，不得写成通过。

## GUI 菜单/添加地址无响应修复（2026-11-16）

用户反馈：顶部菜单点击无反应；地址栏填入 `https://youtu.be/l7sCL3geF0U` 后点击“添加地址 (Add URL)”无反应。

根因：

- `crates/app/src/app.rs` 原第 8 行的 `任务(T) 文件(F) 下载(D) 查看(V) 帮助(H)` 是静态 `ui.label`，不是 egui 菜单按钮；“选项 / 计划任务 / 开始队列 / 停止队列 / 站点抓取”同样只是静态 label。
- “添加地址”按钮原先只把 `status_message` 改成一行小字，没有打开 UI-02/UI-03 窗口，也没有调用 `TaskCoordinator::add_task`；用户操作没有可见结果。

修复（提交 `8b70ee9` + `490af44`）：

- `crates/app/src/app.rs`：把顶部占位文字改为五个真实 `ui.menu_button`，每个菜单项通过 `AppAction` 分发；把工具栏静态 label 改为带禁用状态真实按钮；任务表支持单击/Ctrl/Shift 多选、右键菜单和状态更新。
- `crates/app/src/ui.rs`：新增 `AppAction`、`UiDialog` 与 UI 状态字段；地址栏已有 URL 时，点击“添加地址”会直接调用 `prepare_add_url` 打开“下载信息”窗口；确认后调用唯一状态写入者 `TaskCoordinator::add_task`，YouTube 地址通过 `extract_youtube_identity` 绑定 `platform_media_id` 与 `MediaIntent`。
- “恢复 / 停止 / 开始队列 / 停止队列 / 移除记录”通过 `TaskCoordinator::control_task` 改任务状态，不再只改提示文字。
- 地址栏限制 280px 并加入提示文字，避免按钮在 1024 宽度下被挤出视野。
- 新增 `crates/app/tests/ui_actions.rs`：4 项真实 Rust 测试，覆盖 YouTube URL 解析、任务创建、空 URL 拒绝、菜单动作分发和“地址已有内容时一键进入下载信息窗”。

验证：

- 本地 `cargo +stable check --workspace --all-targets` 通过。
- 本机 `cargo test` 仍因缺少 `dlltool.exe` 无法链接测试二进制；真实测试放 GitHub Actions。
- `wbhy4falshv1/main` CI run **43864085628**：4/4 jobs success；Windows job **58 passed / 0 failed**，其中新增 UI 测试 **4 passed / 0 failed**。证据：`evidence/CI-43864085628/RESULTS.md`。
- 该结果证明编译、格式、Clippy、58 项自动化测试和契约套件通过；**不等于** GUI 真机点击验收通过。用户截图所指具体窗口仍需在新版二进制上真机确认，GUI 真机继续记为 `needs_env`。
- 文档提交 `d12ad26` 的 CI run 43865245516 同样 4/4 jobs success。
- 为便于直接验证修复版本，基于 `d12ad26` 新建预发布标签 `v0.1.1-ui-fix`；Release 工作流 run 43866171544 success。
- 新 Windows 便携包：`rustcatch-win64-portable-unsigned-test.zip`，SHA-256 `831df8497d4efaaf4d40c7e2246a38601be347c953ffd88185b624f9dbb5fd74`，地址：https://github.com/northwaterzuma/wbhy4falshv1/releases/tag/v0.1.1-ui-fix

## 下载执行链路与诊断日志（2026-11-16）

用户第二轮截图：任务已被创建并显示 `Running | 0 B`，但没有真实下载，也看不到信息/日志。

根因（已确认）：

- `TaskCoordinator::control_task(Resume/Retry)` 只把任务改成 `Running`，全工程没有任何 `Command::spawn` / `tokio::process` 调用；`apply_worker_event` 只有消费端，没有生产端。
- `crates/tools/src/proc.rs::build_ytdlp_command`、`components.rs::verify_ready_for_execution`、`ytdlp_errors.rs::classify_ytdlp_stderr` 都是纯函数，只被测试调用，生产代码没有接线。
- `COMPONENT_LOCK.json` 仍是 `unresolved_do_not_execute`，`component_set_id=null`、所有组件 `sha256=null`；按规格禁止执行未核验工具。

本轮修复：

- 新增 `crates/app/src/runner.rs`：后台线程接收 `StartRequest`，先执行组件闸门；若组件已核验，则调用 `build_ytdlp_command` 生成受控 argv，用 `Command` 启动 yt-dlp，独立消费 stdout/stderr，解析进度字节和错误分类，再发 `WorkerEvent`。
- `crates/app/src/model.rs` 增加 `worker_source_url`，保留原始 URL 供 worker 使用；快照和日志仍只显示脱敏 URL。
- `crates/app/src/task_coordinator.rs` 增加 `mark_task_failed`，worker 失败时按 `attempt_id` 写入 `Failed`、错误码和原因，避免旧 attempt 污染状态。
- `crates/app/src/ui.rs` / `app.rs`：新增任务日志窗口、工具栏“日志”、查看菜单“任务日志”、右键“日志”；下载信息窗显示实时状态、attempt、已接收字节、错误码和原因；状态栏显示最近一条日志；有运行任务时定时请求重绘以刷新进度。
- 当前组件锁未解析，因此新包的真实行为是：任务列表会显示 `Failed / ToolUnavailable`，日志明确写出 `COMPONENT_LOCK status is unresolved_do_not_execute` 和缺少已验证 yt-dlp/EJS/Deno/FFmpeg。**这仍是诊断包，不是可下载包**。
- 若要真正下载，下一步必须在 GitHub Actions 发布流程里解析并核验官方组件、写入 `COMPONENT_LOCK.json` 并随包分发，或由用户显式配置“用户提供的工具路径”；不能伪造 SHA-256 或直接调用系统 PATH 里的 yt-dlp。
- 本地 `cargo +stable check --workspace --all-targets` 通过。
- `wbhy4falshv1/main` CI run **43870407465**：4/4 jobs success；Windows job **60 passed / 0 failed**（新增 runner 测试 1 项、UI 测试 5 项）。证据：`evidence/CI-43870407465/RESULTS.md`。
- 诊断版预发布标签 `v0.1.2-diagnostics`（指向 `14412c9`）；Release workflow run **43871553149** success。Windows 便携包 SHA-256 `2ac4f7ec81ee742b937461c99e3318531d87ceb46a3f4c91bc4bafabaaec864e`，地址：https://github.com/northwaterzuma/wbhy4falshv1/releases/tag/v0.1.2-diagnostics

## 组件随包分发与正式发布（2026-11-16T13:33Z）

用户日志确认 `v0.1.2-diagnostics` 的唯一阻塞是 `COMPONENT_LOCK status is unresolved_do_not_execute`。本轮按“不伪造哈希、不调用系统 PATH”的规格约束，在 GitHub Actions 发布流程中解析并核验真实组件，而不是改仓库基线锁文件。

本轮改动：

- `.github/workflows/release.yml`：从 yt-dlp、Deno、BtbN FFmpeg 官方 release 下载资产；yt-dlp 用官方 `SHA2-256SUMS` 比对；所有组件用 `Get-FileHash -Algorithm SHA256` 计算包内真实哈希；生成 `resolved-COMPONENT_LOCK.json`；组件复制到 `dist/rustcatch-win64/components/`；用 resolved 锁替换包内 `COMPONENT_LOCK.json`。
- `crates/app/src/runner.rs`：运行时优先读取 exe 同目录 `COMPONENT_LOCK.json`；输出模板去掉扩展名，避免 yt-dlp 自动补扩展变成双重扩展。
- `crates/tools/src/proc.rs`：受控 argv 加入 `--js-runtimes deno:<locked deno path>`；`--proxy` 仅在本地 relay URL 非空时加入。
- `crates/tools/tests/streams_and_proc.rs`：断言 Deno 运行时参数确实进入受控 argv。

验证证据：

- `wbhy4falshv1/main` CI run **43873418303**：4/4 jobs success；Windows、Ubuntu、macOS 三平台均 **60 passed / 0 failed**。
- Release dry-run run **43875366720**：`workflow_dispatch` 在 `main` 上成功完成组件解析、哈希核验、打包和 artifact 上传；未发布 Release。
- 正式标签 `v0.1.3-components` 指向 `96f48dc`；Release workflow run **43876092254**：所有步骤 success，包括 `Publish to GitHub Release`。
- 发布资产 `rustcatch-win64-portable-unsigned-test.zip`：大小 203,283,038 bytes；GitHub digest 与本地复算 SHA-256 均为 `a5b5e29e2534fdf168b666a3ef2d6f72f7ed2354e2a82fe528dd911af983f6fa`。
- 包内 `COMPONENT_LOCK.json` 实测 `status = resolved_verified`，组件集 `ytdlp-2026.10.26-deno-v2.9.8-ffmpeg-n9.0`；包内 yt-dlp、Deno、FFmpeg、ffprobe 实测 SHA-256 与锁文件逐项一致。
- 完整证据：`evidence/RELEASE-v0.1.3-components/RESULTS.md`。
- Release 地址：https://github.com/northwaterzuma/wbhy4falshv1/releases/tag/v0.1.3-components

行为边界：

- `ToolUnavailable / unresolved_do_not_execute` 的发布包输入问题已经解决。
- 仍需用户在 Windows 真机解压运行、输入 `https://youtu.be/l7sCL3geF0U` 并观察日志，才能确认在线 YouTube 下载真正完成；在此之前 TEST-06 系列仍为 needs_env，不能写成通过。

## 下载可用性修复（2026-11-16T14:10Z）

用户用 `v0.1.3-components` 真机下载成功，但报告三个问题。用用户指定目录 `C:\00github rust\rustcatch-win64-portable-unsigned-test` 的真实文件 + 包内 ffprobe 验证根因：

- "没声音"：用户打开的是 `youtube_l7sCL3geF0U.f399.mp4`（yt-dlp 纯视频分片，只有 av1 视频）；真正的 `youtube_l7sCL3geF0U.webm` 含 av1 视频 + opus 音频，1920x1080 / 493.6 秒，是有声音的合并成品。分片残留是因为 `--keep-video` 没有在 Rust 验证后清理。
- "下载到程序根目录"：`output_template` 空目录时返回裸文件名，worker cwd 即 exe 目录。
- "已完成/未完成看不到东西"：中心列表从未按 `selected_category` 过滤，分类只改高亮。

本轮修复（`49fb35c`）：

- `runner.rs::finalize_output`：成功后用包内 ffprobe 校验成品音轨，删除 `<stem>.f<digits>.*` 分片，日志输出成品绝对路径、音轨有无、清理数量。
- `runner.rs::resolve_output_dir` + `ui.rs::default_download_directory`：默认下载到 exe 同目录 `Downloads/`，UI-03 预填该路径，用户可改。
- `ui.rs::task_matches_category` + `app.rs`：按状态/类型/队列过滤任务列表，侧栏显示各分类实时计数，空分类给出提示。
- 新增 5 项真实 Rust 测试（3 runner + 2 ui）。

验证与发布：

- 主 CI run **43881259247**：4/4 jobs success；Windows、Ubuntu、macOS 各 **65 passed / 0 failed**。
- 标签 `v0.1.4-ui-understandability` 指向 `49fb35c`；Release workflow run **43882920447** success，资产已挂 Release。发布资产 SHA-256 `1925e68c33ca961fe7d80400be3ef92c459bd028293ab5791861e92d4d7102cf`，地址：https://github.com/northwaterzuma/wbhy4falshv1/releases/tag/v0.1.4-ui-understandability
- 证据：`evidence/RELEASE-v0.1.4-ui-understandability/RESULTS.md`。

边界：v0.1.4 的 GUI 最终效果仍需用户下载新包真机确认；分片删除只删除本次任务输出目录下匹配 `<stem>.f<digits>.*` 的文件，不碰其他文件。

## 微博短链访客校验跳转修复（2026-11-16T14:43Z）

用户输入 `http://t.cn/AXTh2EWk` 报 `Unsupported URL` 并被误分类为 `NetworkError`。用打包 yt-dlp 实测：短链经 `video.weibo.com/show?fid=...` 跳到 `passport.weibo.com/visitor/visitor?...&url=<编码真实地址>`，yt-dlp 把网关页当目标用 generic 提取器失败；直接喂还原后的 `https://weibo.com/tv/show/1034:5364596685602859` 则 `[WeiboVideo]` 成功解析出带音频的 mp4。

本轮修复（`d4de8b4`）：

- `tools::video::extract_weibo_visitor_target`：纯函数解析 `Following redirect to <url>`，从 visitor 网关 `url=` 参数还原真实地址，并校验目标 host 必须是 weibo.com 或其子域（防恶意域跳转）。
- `runner::handle_request`：抽出 `run_once` 保留 stdout 行；首次失败后扫描重定向行，还原出不同地址则用同一受控 argv 构造器重试一次，日志写明还原过程。
- 新增 2 项 Rust 单测（真实日志行还原 + 非目标/恶意目标忽略）。

验证与发布：

- 主 CI run **43886518075**：4/4 jobs success；Windows、Ubuntu、macOS 各 **67 passed / 0 failed**。
- 标签 `v0.1.5-weibo-redirect` 指向 `d4de8b4`；Release workflow run **43888160976** success，资产 SHA-256 `865385b99b368ca80cfc03f941955a2d9612afa105f28404c6329e6706bd32ca`，地址：https://github.com/northwaterzuma/wbhy4falshv1/releases/tag/v0.1.5-weibo-redirect
- 证据：`evidence/RELEASE-v0.1.5-weibo-redirect/RESULTS.md`。

边界：本次只解决"访客校验网关跳转"这一已确认失败模式；其它微博反爬/登录墙场景不在范围，v0.1.5 真机端到端下载仍需用户确认。

