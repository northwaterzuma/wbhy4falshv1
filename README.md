# RustCatch Download Manager

RustCatch 是一个采用 Rust 2024、egui/eframe 和 Tokio 的桌面下载管理器项目。产品目标与验收要求见 [开发规格](RUST_DOWNLOAD_MANAGER_SPEC.md)。

## 当前可用范围

- 桌面可新建媒体任务，并从受控组件包调用 yt-dlp、Deno、FFmpeg 和 ffprobe。成品在专用目录暂存，通过轨道检查后同卷提交，同名文件保留。Linux/macOS 当前使用同卷硬链接提交。
- 桌面支持暂停和取消当前执行。恢复会重新启动媒体下载。任务记录目前保存在进程内存。
- 普通文件的原生 HTTP 下载、断点恢复和跨重启任务存储仍在开发。浏览器接管、CLI 下载命令及 MCP 等待共享任务服务，现阶段会明确返回不可用。

详见 [首发验收状态](docs/LAUNCH_REVIEW.md) 和 [功能差距矩阵](docs/FEATURE_MATRIX.md)。

## 编译与检查

使用 Rust 1.95.0。运行 `cargo test --workspace --locked` 检查 Rust 用例。运行 `python3 tests/run_offline_verification.py` 核对 JSON 语法、夹具摘要及 ZIP CRC。

GitHub 上的 **RustCatch CI (Build, Test & Contract Verification)** 在 Windows、Linux 和 macOS 上运行检查、测试和构建。符合配置的分支推送、拉取请求或手动触发可启动 CI。GitHub Actions 构件与实际现场验收分别记录，发布预览包还需验证其内置组件。
