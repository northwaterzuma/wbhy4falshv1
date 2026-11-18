# v0.1.6-http-resume-handoff 验证结果

- 提交：`d8211db feat: add native HTTP resume and browser handoff`（47 files, +4695/−1008）
- 分支：`rustcatch/http-resume-handoff-20261117`
- 标签：`v0.1.6-http-resume-handoff`
- 主 CI：run `44213586615`，4/4 jobs success；Windows / Ubuntu / macOS 各 **81 passed / 0 failed**（较 v0.1.5 的 67 项增加 14 项，覆盖原生 HTTP 传输、续传、持久化与浏览器接管）
- 规格与夹具 job：success，包含 `node --test tests/browser_handoff.test.mjs`（无浏览器条件下的接管顺序验证）
- 发布流水线：run `44215672193`，success，4m7s；`Resolve and Verify Media Components`、`Publish to GitHub Release` 均通过
- Release 页：https://github.com/northwaterzuma/wbhy4falshv1/releases/tag/v0.1.6-http-resume-handoff
- 资产：`rustcatch-win64-portable-unsigned-test.zip`，204,776,320 bytes，GitHub digest `sha256:0c59860cfad0f333aa7b1e558dd422d52b330186fcd7eea2be1d4fbc7137120a`

## 本轮内容

- 普通文件改由 Rust 原生 HTTP/HTTPS 单流传输，不再依赖外部工具。按强 ETag、最终资源身份、长度与 Range 恢复已同步字节；服务端返回完整 200 时从零重写，避免拼接不同版本。
- 任务、检查点与待提交成品记入独立日志。带查询参数的地址不入普通任务记录，重启后需重新输入。
- Chromium 扩展接入真实 Native Host 与桌面任务账本。仅匹配用户授权的公开 GET；先保存 prepare，再核实浏览器取消，最后持久化 commit。失去反馈时以原任务重试或在桌面明确核对。
- 新增模块：`crates/engine/src/http_transfer.rs`、`crates/engine/src/durable.rs`、`crates/app/src/task_store.rs`、`crates/app/src/tool_process.rs`、`crates/app/src/browser_bridge.rs`，以及真实回环集成测试 `crates/engine/tests/native_http.rs`。

## 发布包内容

- `rustcatch.exe`、`rustcatch-cli.exe`、`rustcatch-mcp.exe`、`rustcatch-nmhost.exe`（x86_64-pc-windows-msvc）
- `extension/` 浏览器扩展
- `components/` 内 yt-dlp / Deno / FFmpeg / ffprobe，随包分发的 `COMPONENT_LOCK.json` 为 `status = resolved_verified`，哈希由流水线对实际文件计算并与官方 `SHA2-256SUMS` 比对

## 证据边界（未标记通过）

- 真实 Chrome / Edge 安装接管与桌面窗口操作仍为 needs_env，只验证到协议、顺序与无浏览器回退。
- 多连接调度、带身份的接管、实际掉电恢复、媒体任务跨重启仍按原规格验收。
- 发布包为未签名便携版，只有 Windows x64 MSVC 一种产物。

## 备注

本地 GitHub 直连（Git HTTPS）本轮持续被代理拦截为 `CONNECT tunnel failed, response 502`，tag 对象与引用经 GitHub REST API 创建；Release 触发与结果均以 Actions 运行记录为准。
