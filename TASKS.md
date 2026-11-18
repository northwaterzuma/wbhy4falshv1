# RustCatch 实施任务

规格版本 1.0。验收依据见主规格、SOURCE_LOCK.json、协议 Schema 和 ACCEPTANCE.md。本表与 tasks.json 记录当前任务状态。文件路径和历史 CI 结果须结合实际用例证据判定。

## 1. 执行范围与完成含义

| scope | 含义 | 停止规则 |
|---|---|---|
| launch | Win10/11 x64 + Edge/Chrome 首发必选 | 必须通过 P4-T03；首发通过后继续 P5 |
| full_parity | IDM 完整目标中的高级功能与剩余差距 | 不因低频而放弃；未完成就保留未完成 |
| optional | Flux 实验适配器 | 可不用，不阻塞首发；不能冒充独立解析冗余 |
| cross_platform | macOS/Linux 平台交付 | 单独验证，不继承 Windows 测试结论 |

用户当前目标为完整开发。**P4-T03 是阶段门槛，不是项目终点。** 只有当前用户明确缩小范围，才能在首发停止。P5 的技术依赖不硬等发布签字，缺少某项真机或账号时仍可推进其他满足依赖的工作。

任务状态只用 not_started / in_progress / needs_env / blocked / done。needs_env 表示实现或某个验证等待必要环境，并不算通过。任务 done 必须满足本任务完成标准；具体 TEST 中本任务负责的子用例通过，不能只凭代码已写或编译成功。基础任务可完成其明确的子用例，不要求尚未实现的整个上层套件提前通过。

P1-T01 已接入原生 HTTP/HTTPS 单流与强 ETag 续传，P1-T02 已接入普通下载和交接账本持久化。多连接、掉电与迁移仍按原任务验收。P2-T01/P2-T04 的公开 GET 接管和显式发送已有生产入口，待真实 Chrome/Edge 注册、权限、下载、回退与扩展休眠测试。运行记录见 [HANDOFF.md](HANDOFF.md)。

## 2. 统一语义与并行边界

运行时状态：HandoffPending、Queued、Resolving、AwaitingUser、Running、Finishing、Pausing、Paused、Cancelling、Cancelled、Failed、Completed。

运行时阶段：Resolve、Select、Transfer、Merge、Verify、Commit。不要另建 Downloading/Merging/AwaitingSelection 等竞争状态；例如选择待处理是 AwaitingUser(reason=selection)，限流等待是 Queued(reason=rate_limited, not_before=...)。

同可信调用方作用域与同 idempotency_key 至多创建一项任务；同键同创建语义返回原 task_id，创建语义不同返回冲突；commit/status/abort只引用该交接，另按op/request_id去重操作。主动重新下载使用新键。浏览器与桌面是两个进程，不能宣称绝对 exactly-once；prepare/commit/status/abort、持久化、幂等和不确定状态核对共同保证可恢复交接。

P1-T02 固定任务/API契约；P2-T03 在此基础上独立固定网络契约，P3-T01 随即可并行实现，不等待主窗视觉细节。UI、CLI、扩展及 MCP 共用已有 App 任务目录，禁止另建下载内核、数据库或队列。路径以固定上游检出后复核，已有等价模块优先复用；若调整路径，在交接中写清旧/新映射。

P2-T02 使用猫抓 API 验证捕获；P2-T04 将必要能力整合为正式单扩展。首发用户只需安装 RustCatch 自己一套扩展，不能把要求用户长期同时安装两套扩展当成完成。

## 3. 任务图

| 任务 | scope | 前置依赖 | 当前状态 |
|---|---|---|---|
| P0-T01 冻结 IDM 界面、行为及完整差距基线 | launch | 无 | needs_env |
| P0-T02 复现固定 MDM 底座与依赖审计 | launch | 无 | needs_env |
| P0-T03 建立离线 fixtures 与故障服务器 | launch | 无 | in_progress |
| P1-T01 实现并验证普通 HTTP 下载正确性 | launch | P0-T02、P0-T03 | in_progress |
| P1-T02 统一任务契约、事件和持久化恢复 | launch | P1-T01 | in_progress |
| P1-T03 把 IDM 风格核心界面绑定到真实任务 | launch | P0-T01、P1-T02 | needs_env |
| P2-T01 实现可信 Native Messaging 和两阶段接管 | launch | P1-T02 | needs_env |
| P2-T02 接入猫抓资源 API 与媒体候选 | launch | P2-T01、P2-T03 | needs_env |
| P2-T03 固定网络、凭据和资源预算契约 | launch | P0-T02、P1-T02 | in_progress |
| P2-T04 整合为用户只安装一套的正式扩展 | launch | P2-T02 | needs_env |
| P3-T01 实现默认 yt-dlp 完整 YouTube 管线 | launch | P1-T02、P2-T03、P0-T03 | needs_env |
| P3-T02 完成格式、音轨、字幕与播放列表交互 | launch | P3-T01 | needs_env |
| P3-T03 评估 Flux 实验性备用适配器 | optional | P3-T01 | in_progress |
| P4-T01 实现队列、计划、分类与批量入口 | launch | P1-T02、P1-T03、P2-T01 | needs_env |
| P4-T02 实现发行、数据迁移与 MCP 边界 | launch | P0-T02、P1-T02 | needs_env |
| P4-T03 完成 Windows 首发集成门槛并继续全量目标 | launch | P2-T04、P3-T02、P4-T01、P4-T02 | needs_env |
| P5-T01 补齐 FTP/FTPS 与高级认证 | full_parity | P0-T01、P1-T02、P2-T03 | needs_env |
| P5-T02 补齐站点抓取与周期同步 | full_parity | P0-T01、P4-T01、P2-T03 | needs_env |
| P5-T03 补齐 ZIP 预览及 IDM 剩余差距 | full_parity | P0-T01、P1-T02、P4-T01 | needs_env |
| P5-T04 完成 macOS/Linux 平台适配与全量评审 | cross_platform | P1-T03、P2-T04、P3-T02、P4-T01 | needs_env |

## 任务详情

### P0-T01 冻结 IDM 界面、行为及完整差距基线

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：无。
- 需求：REQ-01、REQ-13、REQ-14、REQ-15、REQ-16、REQ-17、REQ-18。
- 代码/文件归属：desktop-ui / baseline；docs/baseline/idm/、docs/FEATURE_MATRIX.md。
- 验证：TEST-01A，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/baseline/idm/manifest.json`；`docs/baseline/idm/screenshots/`；`docs/FEATURE_MATRIX.md`。
- 下一步：P1-T03、P5-T03。

完成标准：

1. 记录实际 IDM 版本、来源哈希、系统/浏览器版本、语言、主题和 DPI，采集 UI-01～12 的行为证据。
2. 把官方文档与实机差异、所有未完成高级功能逐项写入 REQ 矩阵；未取得实机的项标 needs_env，不伪造测量。
3. 完成可复核的窗口/快捷键/关闭行为清单；TEST-01A 通过后保存证据。


### P0-T02 复现固定 MDM 底座与依赖审计

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：无。
- 需求：REQ-02、REQ-06、REQ-11。
- 代码/文件归属：architecture / build；Cargo.toml、Cargo.lock、rust-toolchain.toml、SOURCE_LOCK.json 对应实现副本、docs/baseline/UPSTREAM_AUDIT.md。
- 验证：TEST-11A，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/baseline/UPSTREAM_AUDIT.md`；`docs/baseline/BUILD_BASELINE.md`；`docs/baseline/REUSE_REGISTER.md`；`COMPONENT_LOCK.json`。
- 下一步：P1-T01、P2-T03、P4-T02。

完成标准：

1. 检出 SOURCE_LOCK.json 指定提交并验证对象类型/SHA；保留上游锁文件，记录构建命令和实际工具链。
2. 审计复用代码与二进制许可证/来源；区分源代码已观察、编译成功和运行验证，选择性回移有来源提交。
3. TEST-11A 基线构建与现有可执行测试有真实结果；依赖不兼容有修复或准确阻塞说明，不能悄悄跟随 main。


### P0-T03 建立离线 fixtures 与故障服务器

- 当前状态 in_progress。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：无。
- 需求：REQ-02、REQ-03、REQ-05、REQ-06、REQ-09、REQ-11。
- 代码/文件归属：release-test；tests/fixtures/、tests/faults/、现有 crates/labserver 等价目录、CI 测试定义。
- 验证：TEST-02A，操作见 ACCEPTANCE.md。
- 计划产物与证据：`tests/fixtures/manifest.json`；`tests/fixtures/media/`；`tests/faults/scenarios.json`；`docs/TEST_EVIDENCE_FORMAT.md`。
- 下一步：P1-T01、P3-T01。

完成标准：

1. 本地生成确定字节文件及合成 HLS/DASH，记录长度/哈希/轨道元数据，测试不依赖公开视频。
2. 故障入口能确定性切换 Range、断流、验证器变化、429、子进程退出与磁盘故障；固定种子和触发点。
3. TEST-02A 验证服务器实际响应与场景声明一致，并建立 not_started/needs_env 的结果记录方式。


### P1-T01 实现并验证普通 HTTP 下载正确性

- 当前状态 in_progress。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P0-T02、P0-T03。
- 需求：REQ-02。
- 代码/文件归属：engine-recovery；crates/engine/src/download.rs、planner.rs、state.rs。
- 验证：TEST-02B，操作见 ACCEPTANCE.md。
- 计划产物与证据：`crates/engine/`；`evidence/P1-T01/`；`docs/HTTP_CAPABILITIES.md`。
- 下一步：P1-T02。

完成标准：

1. 实际验证 206/200/416、长度/验证器、identity 编码及分段覆盖；拒绝错误响应写入，安全降级到单连接。
2. 动态分段、重试和限速接入已有引擎；未知长度和不可续传的能力明确，不建立第二下载内核。
3. TEST-02B 全部通过且最终文件哈希一致；失败无错误 Completed。


### P1-T02 统一任务契约、事件和持久化恢复

- 当前状态 in_progress。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P1-T01。
- 需求：REQ-03、REQ-04、REQ-06、REQ-12。
- 代码/文件归属：architecture / coordinator；crates/app/src/model.rs、task_coordinator.rs（拟新增）、crates/ipc/src/task_api.rs（拟新增）及既有状态存储。
- 验证：TEST-03A、TEST-03B、TEST-12B，操作见 ACCEPTANCE.md。
- 计划产物与证据：`crates/app/src/task_coordinator.rs 或等价模块`；`crates/ipc/src/task_api.rs 或等价类型`；`docs/TASK_CONTRACT.md`；`evidence/P1-T02/`。
- 下一步：P1-T03、P2-T01、P2-T03、P4-T02。

完成标准：

1. 所有入口共用指定状态与阶段；task_id 稳定、attempt_id 更新、旧事件被拒绝，Completed 仅在 Commit 成功落盘后成立。
2. 同可信调用方及同幂等键至多一任务；同键同创建语义返回同任务，创建语义不同冲突；commit/status/abort不与prepare的不同payload作创建摘要比较；控制、删除记录、删除文件语义分离。
3. TEST-03A/03B/12B 通过：检查点、重命名崩溃窗口、取消进程树和 schema 迁移不丢失已确认数据。


### P1-T03 把 IDM 风格核心界面绑定到真实任务

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P0-T01、P1-T02。
- 需求：REQ-01、REQ-03。
- 代码/文件归属：desktop-ui；crates/app/src/ 的窗口/控件模块；app.rs 由协调负责人合并。
- 验证：TEST-01B、TEST-01C，操作见 ACCEPTANCE.md。
- 计划产物与证据：`crates/app/`；`evidence/P1-T03/ui/`；`docs/UI_DIFFERENCES.md`。
- 下一步：P4-T01、P5-T04。

完成标准：

1. 主窗、添加、进度、完成、属性及关键设置复刻冻结基线；状态、进度、分段图来自真实模型。
2. 正确区分关闭窗口/退出、暂停/取消、移除记录/删除文件；键盘与 DPI 无关键遮挡。
3. TEST-01B/01C 对首发窗口通过；全量高级窗口仍留在矩阵，不能用空实现声称完成。


### P2-T01 实现可信 Native Messaging 和两阶段接管

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P1-T02。
- 需求：REQ-04。
- 代码/文件归属：browser-capture；extension/background.js、crates/nmhost/、crates/ipc/ 的桥接入口。
- 验证：TEST-04A、TEST-04B、TEST-04C，操作见 ACCEPTANCE.md。
- 计划产物与证据：`extension/`；`crates/nmhost/`；`docs/HANDOFF_PROTOCOL.md`；`evidence/P2-T01/`。
- 下一步：P2-T02、P4-T01。

完成标准：

1. 校验真实扩展身份、协议版本、帧大小与字段；消息只携带元数据，重试使用同幂等键。
2. prepare 持久化 HandoffPending 且不传输；确认浏览器取消后 commit 才入队，超时/响应丢失先查询状态，结果不明进入 AwaitingUser。
3. TEST-04A/04B/04C 通过；没有把跨进程交接描述为绝对 exactly-once；不确认所有权前不双端启动。


### P2-T02 接入猫抓资源 API 与媒体候选

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P2-T01、P2-T03。
- 需求：REQ-05、REQ-09。
- 代码/文件归属：browser-capture / media；extension/ 捕获模块、crates/app/src/capture.rs（拟新增）、crates/tools/src/streams.rs。
- 验证：TEST-05A、TEST-05B，操作见 ACCEPTANCE.md。
- 计划产物与证据：`extension/ 捕获模块`；`crates/app/src/capture.rs 或等价模块`；`docs/CAPTURE_MODEL.md`；`evidence/P2-T02/`。
- 下一步：P2-T04。

完成标准：

1. 开发阶段通过已核实猫抓 API 获取当前标签页候选，按导航代次、来源、manifest 和轨道身份合并，不靠标题或 URL 裁剪去重。
2. HLS/DASH 关联、分页、容量限制、权限上下文和受保护资源提示有效；未知 blob/MSE 不虚报可下载。
3. TEST-05A/05B 通过，捕获候选与真实媒体任务只经过统一提交入口。


### P2-T03 固定网络、凭据和资源预算契约

- 当前状态 in_progress。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P0-T02、P1-T02。
- 需求：REQ-09、REQ-06、REQ-08。
- 代码/文件归属：architecture / network；crates/engine/ 的网络策略、crates/tools/src/media_policy.rs（拟新增）、可信上下文与系统凭据适配层。
- 验证：TEST-09A、TEST-09B、TEST-09C，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/NETWORK_CONTEXT.md`；`crates/tools/src/media_policy.rs 或等价模块`；`evidence/P2-T03/`。
- 下一步：P2-T02、P3-T01、P5-T01。

完成标准：

1. 固定 NetworkProfile、ContextHandle、代理/DNS/认证范围及版本；代理失败不静默直连，未知 PAC 不冒充与浏览器一致。
2. 所有媒体阶段和工具更新使用明确路线；跨域重定向不泄露认证，凭据不进入 DTO/日志/MCP。
3. TEST-09A/09B/09C 通过并发布接口；YouTube 可据此并行开发，不等待主窗视觉细节。


### P2-T04 整合为用户只安装一套的正式扩展

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P2-T02。
- 需求：REQ-04、REQ-05、REQ-11。
- 代码/文件归属：browser-capture / release；extension/manifest.json、捕获与浮条模块、扩展打包/升级配置。
- 验证：TEST-05C、TEST-04C，操作见 ACCEPTANCE.md。
- 计划产物与证据：`dist/extension/（实现后生成）`；`docs/EXTENSION_INSTALL.md`；`docs/baseline/REUSE_REGISTER.md 的移植记录`；`evidence/P2-T04/`。
- 下一步：P4-T03、P5-T04。

完成标准：

1. 保留猫抓 API 桥接为开发验证路线；首发将必要捕获能力按许可整合进本产品扩展，运行不依赖另装猫抓。
2. 本产品扩展使用独立 ID/公钥与 Native Host，避免重复浮条和接管；原猫抓可并存但不是必需依赖。
3. 干净 Edge/Chrome 仅安装本产品扩展即通过 TEST-05C 和 TEST-04C，权限、更新、来源声明及安装包齐全。


### P3-T01 实现默认 yt-dlp 完整 YouTube 管线

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P1-T02、P2-T03、P0-T03。
- 需求：REQ-06、REQ-09。
- 代码/文件归属：media-pipeline；crates/tools/src/video.rs、proc.rs、media_policy.rs 及组件管理器。
- 验证：TEST-06A、TEST-06B、TEST-06C、TEST-06D、TEST-06E、TEST-06F，操作见 ACCEPTANCE.md。
- 计划产物与证据：`crates/tools/`；`COMPONENT_LOCK.json`；`docs/YOUTUBE_ERROR_MAP.md`；`tests/fixtures/youtube-manifest.json`；`evidence/P3-T01/`。
- 下一步：P3-T02、P3-T03。

完成标准：

1. 受控 yt-dlp + 匹配 EJS + 独立 runtime + FFmpeg/ffprobe 管理解析、传输和合并；固定 component_set_id，运行中不替换。
2. 明确登录、Token能力、限流、格式消失、解析器变化及本地失败；有预算的恢复，不把所有403都当网络错误。
3. TEST-06A～F 通过；在线缺环境标 needs_env；断网/组件修复与回滚不丢任务，不等待 UI 细节才能开发。


### P3-T02 完成格式、音轨、字幕与播放列表交互

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P3-T01。
- 需求：REQ-07、REQ-01。
- 代码/文件归属：media-pipeline / desktop-ui；crates/tools/src/media_policy.rs、媒体选择 DTO、crates/app/ 视频窗口。
- 验证：TEST-07A、TEST-07B、TEST-07C、TEST-07D，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/MEDIA_SELECTION.md`；`crates/tools/ 格式选择模块`；`crates/app/ 视频窗口`；`evidence/P3-T02/`。
- 下一步：P4-T03、P5-T04。

完成标准：

1. 保存画质/编码/语言意图，区分精确格式与上限；原声、配音、人工/自动字幕有明确标识。
2. Shorts、列表预览/选择/分页/重复处理有效；任务大小估算不作为 Range 边界；格式消失不静默降级。
3. TEST-07A～D 通过，最终媒体轨道与用户选择一致，字幕缺失或合并失败不伪装完整成功。


### P3-T03 评估 Flux 实验性备用适配器

- 当前状态 in_progress。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P3-T01。
- 需求：REQ-06、REQ-07。
- 代码/文件归属：media-pipeline（独立适配器负责人）；crates/tools/ 的 feature-gated adapter；不得修改默认解析策略。
- 验证：TEST-06F、TEST-07D，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/FLUX_EVALUATION.md`；`crates/tools/ 可选适配器`；`evidence/P3-T03/`。
- 下一步：P4-T03（非依赖，可同步报告）。

完成标准：

1. 先核对 AGPL 等许可与复用边界；无可接受代码复用方案时以行为参考和评估报告结束，不偷换许可。
2. 能力置于默认关闭开关；共享 EJS 风险明确，不宣称独立解析冗余；不得用切换绕过限流/权限/保护。
3. TEST-06F/07D 的适用用例有结果，失败可完全禁用；主路径和首发不依赖该任务。


### P4-T01 实现队列、计划、分类与批量入口

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P1-T02、P1-T03、P2-T01。
- 需求：REQ-08、REQ-10、REQ-01。
- 代码/文件归属：desktop-ui / coordinator；crates/app/src/queue.rs、scheduler.rs、分类与批量窗口；限速实现由 network 负责人合并。
- 验证：TEST-08A、TEST-08B、TEST-10A、TEST-10B、TEST-01B，操作见 ACCEPTANCE.md。
- 计划产物与证据：`crates/app/ 管理模块`；`docs/SCHEDULER_POLICY.md`；`docs/IMPORT_EXPORT.md`；`evidence/P4-T01/`。
- 下一步：P4-T03、P5-T02、P5-T04。

完成标准：

1. 多队列、分类、批量、剪贴板、拖放与统一任务目录联通，导入导出不泄露凭据。
2. 计划保存时区与 occurrence_id，睡眠/DST规则明确；统一限速实测覆盖原生及外部工具，否则不标全局限速通过。
3. TEST-08A/08B/10A/10B 与对应界面通过，队列取消/暂停后不偷偷再启动。


### P4-T02 实现发行、数据迁移与 MCP 边界

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P0-T02、P1-T02。
- 需求：REQ-11、REQ-12。
- 代码/文件归属：release-test / MCP；安装器、更新配置、crates/mcp/、crates/nmhost/ 注册逻辑；共享 DTO 由架构负责人合并。
- 验证：TEST-11B、TEST-11C、TEST-12A、TEST-12B，操作见 ACCEPTANCE.md。
- 计划产物与证据：`dist/（实现后生成）`；`docs/INSTALL_UPDATE_ROLLBACK.md`；`THIRD_PARTY_NOTICES`；`docs/MCP_PERMISSIONS.md`；`evidence/P4-T02/`。
- 下一步：P4-T03。

完成标准：

1. 产品身份、数据目录、IPC/Native Host、更新地址和签名公钥与上游分离；安装/卸载不误删用户文件。
2. MCP复用统一命令，结构化 argv 与服务端能力校验；默认无删除文件/任意命令权限，返回脱敏诊断。
3. TEST-11B/11C/12A/12B 通过；签名缺失可交内部测试包但不得称已签名或已公开发布，正式分发另有实际授权。


### P4-T03 完成 Windows 首发集成门槛并继续全量目标

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P2-T04、P3-T02、P4-T01、P4-T02。
- 需求：REQ-01、REQ-02、REQ-03、REQ-04、REQ-05、REQ-06、REQ-07、REQ-08、REQ-09、REQ-10、REQ-11、REQ-12。
- 代码/文件归属：integration / release-test；发行证据、差距矩阵、安装包整合；不借验收重写模块。
- 验证：TEST-01、TEST-02、TEST-03、TEST-04、TEST-05、TEST-06、TEST-07、TEST-08、TEST-09、TEST-10、TEST-11、TEST-12，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/LAUNCH_REVIEW.md`；`docs/FEATURE_MATRIX.md`；`dist/`；`evidence/release/`；`HANDOFF.md`。
- 下一步：P5-T01、P5-T02、P5-T03、P5-T04。

完成标准：

1. 所有 launch 任务及 REQ-01～12 必选用例通过；Win10/11、Edge/Chrome、授权在线样本有真实证据，无发布阻断缺陷。
2. 性能对照记录条件、样本和不确定性，不能凭 Rust/线程数声称快5倍；未完成高级功能准确显示在差距矩阵。
3. 交付可复现源码、包、组件许可、哈希、操作与回滚说明；首发后立即继续 P5，除非当前用户明确将范围缩为首发。


### P5-T01 补齐 FTP/FTPS 与高级认证

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P0-T01、P1-T02、P2-T03。
- 需求：REQ-13、REQ-14。
- 代码/文件归属：engine-recovery / OS-auth；crates/engine/ 协议适配器、隔离的系统认证适配层、对应设置页面。
- 验证：TEST-13A、TEST-13B、TEST-14A，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/PROTOCOL_AUTH_MATRIX.md`；`crates/engine/ 协议与认证适配器`；`evidence/P5-T01/`。
- 下一步：P5-T02、P5-T03、P5-T04、最终全量评审。

完成标准：

1. 在受控服务验证 FTP/FTPS 的模式、REST恢复、TLS校验、会话限制及代理支持；SFTP不冒充FTP。
2. 使用受控测试域验证目标 NTLM/Kerberos/Negotiate 组合；不能以依赖编译通过替代SSO实测。
3. TEST-13A/13B/14A通过并消除该范围差距；缺环境保留needs_env继续其他P5任务，不静默取消。


### P5-T02 补齐站点抓取与周期同步

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P0-T01、P4-T01、P2-T03。
- 需求：REQ-15、REQ-16。
- 代码/文件归属：coordinator / desktop-ui；现有抓取与调度等价模块、四步向导、持久化项目模型。
- 验证：TEST-15A、TEST-16A，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/GRABBER_SYNC_POLICY.md`；`crates/app/ 抓取与同步模块`；`evidence/P5-T02/`。
- 下一步：P5-T03、P5-T04、最终全量评审。

完成标准：

1. 四步抓取向导、范围/深度/页数/字节限制、过滤、项目恢复与离线链接改写联通真实引擎。
2. 周期同步区别于普通队列：验证器比较、暂存校验再替换、备份/冲突规则明确，远端删除不默认删本地。
3. TEST-15A/16A通过；动态页面等能力不足如实登记并继续补齐目标，不能把静态HTML抓取称完整浏览器爬虫。


### P5-T03 补齐 ZIP 预览及 IDM 剩余差距

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P0-T01、P1-T02、P4-T01。
- 需求：REQ-17、REQ-18、REQ-01。
- 代码/文件归属：engine-recovery / desktop-ui / OS；ZIP预览适配器、完成动作、基线中未实现窗口与协议。
- 验证：TEST-17A、TEST-18A、TEST-01B，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/ZIP_PREVIEW_POLICY.md`；`docs/FEATURE_MATRIX.md`；`docs/FULL_PARITY_REVIEW.md`；`evidence/P5-T03/`。
- 下一步：P5-T01、P5-T02、P5-T04、最终全量评审。

完成标准：

1. ZIP/ZIP64预览按Range读取受限目录；不支持、加密、分卷等反馈准确，不默认完整下载作预览。
2. 逐条关闭基线剩余差距，包括被确认存在的旧协议/入口/完成动作；无法完成的项保留未完成，不以延期声明任务done。
3. TEST-17A/18A及新增高级窗口TEST-01B通过；完整对齐只有在REQ-13～18全部满足后才成立。


### P5-T04 完成 macOS/Linux 平台适配与全量评审

- 当前状态 needs_env。验收进展见 docs/FEATURE_MATRIX.md 与 HANDOFF.md。
- 依赖：P1-T03、P2-T04、P3-T02、P4-T01。
- 需求：REQ-19。
- 代码/文件归属：platform / release-test；平台安装器、Native Host注册、系统凭据/代理/托盘等平台层；复用现有核心。
- 验证：TEST-19A，操作见 ACCEPTANCE.md。
- 计划产物与证据：`docs/PLATFORM_MATRIX.md`；`dist/ 对应平台产物`；`docs/PROJECT_COMPLETION_REVIEW.md`；`evidence/P5-T04/`。
- 下一步：最终全量评审、修复评审中仍开放的REQ。

完成标准：

1. macOS/Linux逐平台验证安装、桥接、代理、凭据存储、媒体组件、DPI及桌面能力，明确发行格式与实际验证环境。
2. TEST-19A以及各平台适用TEST-01～18通过；无法支持功能仍保留差距，不把Windows通过结论直接复制。
3. 更新全项目评审：launch、full_parity、cross_platform均完成且无阻断项才称约定项目完成；optional未采用可保留明确评估结论。

## 4. 证据、交接与阻塞处理

P4-T03 的直接依赖覆盖全部首发必选叶子：P2-T04、P3-T02、P4-T01、P4-T02；其祖先覆盖其他 launch 任务。它不依赖 P3-T03，也不依赖 P5。源码合并后必须在同一个精确提交重新进行跨模块验证，不能拼接互不兼容提交的通过记录。

每次交接更新实现仓库的 HANDOFF.md，单 Agent 与多 Agent 使用同一格式；这里的示例不是执行结果：

```yaml
project: RustCatch
spec_version: "1.0"
task_id: P0-T01
status: not_started
repo_path: null
branch: null
commit: null
changed_files: []
requirements: []
tests_run: []
tests_not_run: []
evidence_paths: []
decisions: []
blockers: []
next_action: "读取工作区规则，核对 SOURCE_LOCK 和既有状态，开始首个无阻塞任务"
```

多 Agent 使用独立分支/工作树并登记文件所有权。共享 DTO、状态机、Cargo.lock、app.rs 由指定负责人合并；任何人改动契约须同步 Schema、调用方和测试。返回上级的完成消息应包含精确提交、已测/未测项及下一步。

只有缺少必要凭据/授权、真实环境、不可兼容的源版本或实质需求冲突才阻塞受影响部分；常规实现选择、可逆修复和已有授权范围内的验证继续推进。不得因上下文不足、某个在线样本暂不可用或一个发布凭据缺失而停止全部项目。blocked/needs_env 条目写明影响需求、已有证据及最小解除条件，继续其他已满足技术依赖的任务。

最终评审检查 REQ-01～19 的完整矩阵和实际交付；既未实现又仅写成“延期”的 full_parity 项不能算 done。optional 可以不采用；是否对外发布、签名或提交商店按当前实际授权执行，与完成本地开发及测试区分。

## 5. 本轮增量状态（2026-11-16T13:33Z）

- 组件发布链：`已实现，GitHub CI/Release 验证通过`；指向提交 `96f48dc`，tag `v0.1.3-components`。
- 已验证范围：CI run `43873418303` 4/4 jobs success，Windows/Ubuntu/macOS 各 60 passed / 0 failed；Release dry-run `43875366720` success；正式 Release run `43876092254` success。
- 发布包实测：`COMPONENT_LOCK.json` 为 `resolved_verified`，包内含已核验 yt-dlp、Deno、FFmpeg、ffprobe；发布资产 SHA-256 与 GitHub digest 一致。
- 仍未通过：在线 YouTube 真机下载验收（TEST-06 系列）继续 `needs_env`，必须由用户运行新包并确认日志与输出文件后才能改为通过。
- 证据：`evidence/RELEASE-v0.1.3-components/RESULTS.md`；`HANDOFF.md` 组件随包分发与正式发布章节。

