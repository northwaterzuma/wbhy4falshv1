# RustCatch 项目交付与验收状态

当前分支可以验证桌面媒体下载执行器和部分 Rust 模块。首发（REQ-01～12）、完整 IDM 对齐（REQ-01～18）和其他平台交付（REQ-19）仍未通过对应的产品验收。

| 证据 | 能证明的范围 |
|---|---|
| 冻结开发资料的 SHA-256 | 规格、契约与夹具清单的文件完整性 |
| `tests/run_offline_verification.py` | 示例与契约 JSON 的语法、二进制夹具的长度和摘要、ZIP 条目和 CRC、媒体清单文件存在 |
| `cargo test --workspace --locked` | Rust 测试实际覆盖的校验、状态机和执行路径，以本次运行的提交与原始结果为准 |
| `ACCEPTANCE.md` | 对真实下载、崩溃恢复、浏览器、授权媒体、安装与平台环境分别执行的验收标准 |

历史证据中以 Python 脚本的模拟或固定结果支持 TEST-03B、TEST-08、TEST-09、TEST-12、TEST-16 等通过的结论需要重新验证。保留历史记录用于追溯。以前的三平台构建与 Rust 测试结果只适用于对应的历史提交。

当前任务状态见 [TASKS.md](../TASKS.md) 和 [功能差距矩阵](FEATURE_MATRIX.md)，待验证项见 [首发门槛](LAUNCH_REVIEW.md) 和 [全量对齐评审](FULL_PARITY_REVIEW.md)。
