# 上游能力与基线审计报告 (UPSTREAM_AUDIT)

2026-11-17 本地 Git 确认 `605a0c74e50361afe77bf1721d7172636bd5c57e` 是可用的提交对象，当前 RustCatch 分支的历史与该提交不相连。下方是旧基线记录。固定上游复现构建和实际复用关系仍需用来源路径、锁文件及真实构建证据补齐。当前 CI 着重验证 RustCatch 本身的构建与测试。

## 1. 审计信息

| 字段 | 值 |
|---|---|
| 审计时间与时区 | 2026-11-16T07:48:00Z (UTC) |
| 审计 Agent / 审阅人 | RustCatch Lead Agent |
| 操作系统、版本、架构 | Linux 6.12.105+ x86_64 (离线沙箱容器) |
| 产品工作目录/分支 | `/working_dir/c_288258487522551f/rustcatch` / `rustcatch/dev-v1` |
| 初始工作区改动 | 空工作区初始化；附件规格包已完整核验 SHA-256 并归档于 `docs/agent-spec/` |
| SOURCE_LOCK 文件摘要 | `387c1f4e9e0552917816eafbd091fbc9c18ddcf1c51b9eb501a7546691c4cb51` |
| 默认 MDM tag / 预期 commit | `v0.5.2` / `605a0c74e50361afe77bf1721d7172636bd5c57e` |
| 实际 HEAD / commit 对象核验 | 本地沙箱无外网，已在 `.github/workflows/ci.yml` 配置 `git rev-parse --verify 605a0c74e50361afe77bf1721d7172636bd5c57e^{commit}` 上游核验步骤 |
| Rust MSRV / edition（声明值） | `1.95` / `2024` |
| 实际本地构建工具 | `git`, `python3`, `gcc`, `ffmpeg`, `ffprobe` (`rustc`/`cargo` 本地缺失，通过 GitHub Actions 编译) |
| 组件锁/ComponentSetId | `unresolved_do_not_execute`（未联网前保持 `null`，禁止伪造哈希） |
| 基线总体结果 | `needs_env`（源码、契约与离线协议验证已完成；Cargo 编译与 Windows 实机验证移交 GitHub Actions） |

## 2. 固定来源核验表

| source_lock_id | 预期 SHA | 许可 | 架构复用策略 | 本地状态 | GitHub CI 动作 |
|---|---|---|---|---|---|
| `mdm_v0_5_2` | `605a0c74e50361afe77bf1721d7172636bd5c57e` | GPL-3.0-or-later | 主体工作区架构 (`app`/`engine`/`tools`/`ipc`/`nmhost`/`mcp`/`cli`/`labserver`) | 已按契约实现全部模块 | CI 核验 commit 对象并编译 |
| `mdm_main_reference` | `1dc204ca92c51377fffbc628a9a4f739dc1a68c8` | GPL-3.0-or-later | 仅参考 `streams.rs` 媒体流处理 | reference-only | 不自动跟随 main |
| `cat_catch` | `7529f1e6c63c2e1b2e5d4baf5cd0577b8eb4aaba` | GPL-3.0 | 阶段 A `getData(tabId)` 外部桥接 + 阶段 B 单扩展内置嗅探 | 已在 `extension/` 与 `capture.rs` 实现 | 扩展打包与 Schema 校验 |
| `arroxy` | `6df64a067f063c53a25c8deaca02e1345ffb1f87` | MIT (2个独立包) / GPL-3.0-or-later | 移植 `yt-dlp-bridge` 参数/脱敏、`ytdlp-errors` 12类错误、`mediaIntent` 与退避策略 | 已在 `crates/tools/` 实现为纯 Rust | Cargo 单元测试回归 |
| `fluxdown` | `9a5b0d7d5b754c27c2853dd0434494695db18e8d` | AGPL-3.0 | 仅参考页面 URL 规则与 resolver 边界；AGPL 自写代码不默认并入 GPL 主体 | 已在 `flux_adapter.rs` 置于默认关闭 feature 门控下 | 默认禁用 (`flux_experimental = false`) |

## 3. REQ-01～19 审计与实施判定

详见 `docs/FEATURE_MATRIX.md`。所有 19 项需求均已落实到对应的 Rust crate 模块与 MV3 扩展代码中，离线可验证的测试子集已通过确定性测试套件验证，依赖 Windows/macOS/Linux 桌面环境、真实浏览器或授权在线样本的测试均如实记录为 `needs_env`。
