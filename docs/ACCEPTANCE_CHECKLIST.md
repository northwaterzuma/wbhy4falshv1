# RustCatch v0.1.6 真机验收清单（用户操作版）

本清单用于把当前标记为 needs_env 的项目在真实 Windows + 真实浏览器上逐项确认。
对应版本：`v0.1.6-http-resume-handoff`，代码提交 `d8211db`。
每项记录三件事：**做了什么 / 看到什么 / 通过还是不通过**，发回即可更新证据状态。

---

## 一、开始前准备（一次即可）

1. 下载新包（**不要用旧的解压目录**，避免混淆）：
   `https://github.com/northwaterzuma/wbhy4falshv1/releases/download/v0.1.6-http-resume-handoff/rustcatch-win64-portable-unsigned-test.zip`
2. 新建目录并解压，例如：`C:\00github rust\rustcatch-v0.1.6-test`
3. 确认目录里有：`rustcatch.exe`、`rustcatch-cli.exe`、`rustcatch-mcp.exe`、`rustcatch-nmhost.exe`、`extension\` 文件夹、`components\` 文件夹、`COMPONENT_LOCK.json`、`config.toml`
4. 用记事本打开 `COMPONENT_LOCK.json`，确认 `"status": "resolved_verified"`。如果看到 `unresolved_do_not_execute`，说明拿错包了，媒体下载会被组件闸门拒绝。

> 全程请保留程序窗口的「日志 / 任务日志」，出问题时把里面的文字发回。

---

## 二、先看清三类项目

| 类别 | 项目 | 你实际要判断的事 |
|---|---|---|
| A. 功能已实现，真机确认即可 | 窗口与菜单、浏览器接管、掉电恢复（HTTP 部分） | 操作后是否出现预期现象；出问题就是 bug，要修 |
| B. 已知差距，确认程序"如实拒绝" | 带身份接管、多连接 | 预期就是**不发生**；若程序假装成功或错误处理，才是问题 |
| C. 尚未实现，确认差距记录准确 | 媒体跨重启 | 预期是明确提示不可用，不是真的恢复 |

---

## 三、A 类：功能已实现，逐项确认

### 第 1 项：窗口与菜单（对应 TEST-01A 子集）— 建议第一个做

**前置**：双击 `rustcatch.exe`，等主窗口出现。

| 点击位置 | 预期现象 |
|---|---|
| 顶部菜单「任务(T)」→「添加地址... (Ctrl+N)」 | 弹出「添加地址 (Add URL)」窗口，不再是点了没反应 |
| 顶部菜单「文件(F)」→「新建下载任务」 | 同样弹出添加地址窗口 |
| 顶部菜单「查看(V)」→「未完成 / 已完成 / 视频」 | 主列表切换分类；侧栏各类旁边有数量；空分类显示"该分类下暂无任务" |
| 顶部菜单「查看(V)」→「任务日志」；工具栏「日志」 | 打开日志窗口，能看到本次操作记录 |
| 顶部菜单「帮助(H)」→「关于 RustCatch」 | 弹出关于窗口 |
| 工具栏输入框粘贴一个 HTTP/HTTPS 直链 → 点「添加地址 (Add URL)」→「下一步 (Next)」→ 确认 | 主列表出现新任务 |
| 任务行上的「下载信息 / 恢复 / 停止 / 移除记录」按钮 | 分别打开信息窗、改变任务状态、移除记录 |

**怎么算通过**：菜单和按钮都有可见反应；添加地址能建出任务；分类能看到任务。

**重点回归**（你之前反馈过的问题，请特别确认）：
- 菜单是否还会"点了没反应"
- 输入地址后是否还会"点添加地址没反应"
- 「已完成 / 未完成」里是否看得到任务
- 下载是否默认落到 `...\rustcatch-v0.1.6-test\Downloads\` 子目录，而不是散在程序根目录

---

### 第 2 项：真实 Chrome/Edge 接管（对应 TEST-04C / TEST-05C）

**前置**：`rustcatch.exe` 保持运行。

1. Chrome 或 Edge 打开 `edge://extensions`（或 `chrome://extensions`），打开「开发者模式」→「加载已解压的扩展程序」→ 选中 `...\rustcatch-v0.1.6-test\extension` 文件夹。
2. 在扩展卡片上记下 **32 位扩展 ID**。
3. 以管理员以外的普通 PowerShell，进入包目录，执行（把路径和 ID 换成实际值）：

   ```powershell
   cd "C:\00github rust\rustcatch-v0.1.6-test"
   .\rustcatch-cli.exe register-host "C:\00github rust\rustcatch-v0.1.6-test\rustcatch-nmhost.exe" "<你的扩展ID>"
   ```

   输出会给出清单文件的绝对路径，**复制这个路径**。
4. 设置注册表（把上一步路径粘到末尾引号里；Chrome 和 Edge 都要用就各执行一次）：

   ```powershell
   # Edge
   New-Item -Path "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.rustcatch.bridge" -Force | Out-Null
   Set-ItemProperty -Path "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.rustcatch.bridge" -Name "(default)" -Value "<清单绝对路径>"
   # Chrome
   New-Item -Path "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.rustcatch.bridge" -Force | Out-Null
   Set-ItemProperty -Path "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.rustcatch.bridge" -Name "(default)" -Value "<清单绝对路径>"
   ```

5. 点扩展图标打开弹窗，填入一个**确定无需登录、地址里没有 `?` 查询参数、可以重复 GET 的文件直链**，勾选「此链接无需登录，允许重复下载」，点「启用此链接」。
6. 在**同一个网站的页面里**点这个链接下载。也可用链接右键菜单「发送公开文件链接到 RustCatch」。

**预期现象**：浏览器原下载被取消，桌面 RustCatch 主列表出现对应任务并开始下载；弹窗显示交接状态。
**失败现象**：浏览器照常自己下载、桌面无任务；或桌面出现"待核对"且无法继续。
**异常恢复**：弹窗「核对并重试交接」→ 桌面若提示需要核对，**先确认浏览器那边确实取消了**，再点「我已确认浏览器取消，继续下载」；不确定就点「放弃桌面接管」。

> 直链样本请自查可访问（例如公司内网文件、你自己的对象存储公开文件）。公开测试直链可能随时失效，链接本身打不开不算程序问题。

---

### 第 3 项：实际掉电 / 进程异常终止恢复（TEST-03 真机部分）

**更安全做法（推荐先做）**：用一个较大的直链（几百 MB），下载到一半时打开任务管理器，找到 `rustcatch.exe` →「结束任务」，然后重新启动 `rustcatch.exe`。

**预期现象**：桌面能看到该任务存在检查点； Resume/恢复后从已下载字节继续，不从头开始；成品文件哈希/大小正确。
**怎么做证**：记下中断前的进度字节和重启后的进度字节，发回给我。

> 真正拔电源是更强的证据，但可能影响整机文件系统，是否做由你决定；进程强杀已能覆盖同一条恢复路径。

---

## 四、B 类：已知差距，确认程序"如实拒绝"

### 第 4 项：带身份接管

**操作**：在需要登录的网站（内网系统、网盘登录后）点一个文件下载，扩展不作任何授权。
**预期现象（就是通过）**：**浏览器自己下载，RustCatch 不接管**。程序不会去接管带 Cookie / 授权头的请求——这是设计要求，不是故障。
**才算问题**：程序错误接管了需要登录的链接然后失败，或桌面日志里出现完整下载地址/凭据。

### 第 5 项：多连接

**操作**：下载一个大文件，观察程序/系统连接数（Windows 可用 `netstat -ano | findstr <PID>`）。
**预期现象（如实记录）**：当前实现是**原生 HTTP 单流 + Range 续传**，预期只有一条主连接。多线程分段并发调度尚未实现。
**才算问题**：程序界面或文档声称"多连接已支持"却实际没有——那是描述错误，需要改，而不是你去等它变快。

---

## 五、C 类：尚未实现

### 第 6 项：媒体跨重启

**操作**：创建一个 YouTube/媒体任务，下载中途关闭程序再打开。
**预期现象（如实记录）**：普通 HTTP 文件的检查点可恢复；**媒体任务的跨重启恢复尚未实现**，程序应明确说明不可用/需重新提交，而不是假装恢复或误报完成。
**才算问题**：程序假装恢复成功、把分片当成品、或误报 Completed。

---

## 六、结果怎么发回给我

每项给我一行即可，例如：

```
1 窗口菜单：通过。菜单有反应，地址能建任务，落到 Downloads 子目录。截图 1.png
2 浏览器接管：不通过。Edge 里点链接仍自己下载，桌面无任务。扩展 ID=xxxx。日志见末尾
3 掉电恢复：通过。中断 120MB，恢复后从 120MB 继续。
4 带身份：通过（未接管，浏览器自己下）。
5 多连接：确认单流，无多连接。
6 媒体跨重启：确认提示不可用。
```

我会据此把对应项从 needs_env 改为"已验证"，或按你报的现象修 bug、重出包。
