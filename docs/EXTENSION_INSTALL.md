# 浏览器下载接管

桌面程序、Native Host 和一套 RustCatch MV3 扩展共同处理交接。桌面必须正在运行。默认保留浏览器自己的下载流程。

## 配置

1. 在 Chrome 或 Edge 的扩展管理页加载仓库的 extension 目录，记下页面显示的 32 字符扩展 ID。
2. 用同一构建的 CLI 生成 Native Messaging 清单。将下面的路径和 ID 改成实际值。命令输出清单绝对路径。

   ~~~powershell
   .\rustcatch-cli.exe register-host "C:\RustCatch\rustcatch-nmhost.exe" "<扩展ID>"
   ~~~
3. 将该清单注册到正在使用的浏览器。Windows 当前用户的 Chrome 键为 HKCU\Software\Google\Chrome\NativeMessagingHosts\com.rustcatch.bridge，Edge 键为 HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.rustcatch.bridge。键的默认值为上一步输出的清单路径。CLI 只生成清单，浏览器注册需要单独完成。
4. 打开桌面和扩展弹窗，填入一个确定无需登录、无查询参数且可以重复 GET 的完整文件链接，确认并授权该站点。规则只匹配这条地址。用同源页面内的链接测试。
5. 独立发送公开链接可以使用链接右键菜单里的“发送公开文件链接到 RustCatch”。

Linux 和 macOS 的 Chrome/Chromium 使用对应产品的用户 NativeMessagingHosts 目录。把生成的同一清单放入该目录，并保留 RustCatch 数据目录中的副本供 Host 检查来源。首次设置应以一套浏览器配置验证实际扩展 ID 和安装路径。

## 交接与恢复

- 扩展要观察到唯一、对应的普通 GET。重定向、有 Cookie 或授权头、含查询参数、POST、来源无法核对的下载留在浏览器。
- 桌面先落盘待交接任务，再让扩展取消原下载。扩展核实取消结果后才提交。取消不明确时保留待核对记录。
- 弹窗“核对并重试交接”复用原下载标识和幂等键。若桌面显示需要核对，只有确认浏览器原下载已取消后才选择继续。
- 本地验证覆盖模拟浏览器事件、Native Host 身份证明及桌面账本重启。真实 Chrome/Edge 安装、权限与实际下载仍须完成 TEST-04C。
