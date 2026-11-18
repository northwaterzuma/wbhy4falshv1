# 接口契约使用说明

当前公开文件 Host 可在受信的 Native Messaging 连接上接收 `hello.enroll.installation_key`。扩展在私有本地存储生成 32 字节随机值。Host 由实际来源和随机值确定安装标识，并以一次性挑战校验 HMAC。原始值只留在扩展和当前 Host 握手内存中，下载记录只保留安装标识。

这些文件定义目标接口，并非现成 SDK。实现时 Rust 的 serde 类型为唯一源，导出对应 JSON Schema 和扩展 TypeScript 类型；第一次导出须与本包逐项比对，不能通过删约束让测试通过。

| 文件 | 内容 |
|---|---|
| capture_v1.schema.json | Native Messaging 请求包头和按 op 区分的 payload |
| response_v1.schema.json | 成功/失败响应、握手返回与脱敏任务快照 |
| media_intent_v1.schema.json | 格式、画质、字幕、降级与转码意图 |
| network_profile_v1.schema.json | system/manual/direct 网络配置 |

使用 JSON Schema Draft 2020-12。schema ID 采用 rustcatch.invalid 保留域，是标识符，不是需要访问的服务。验证器必须把四份本地 Schema 注册到解析器，禁止为解析引用联网抓取。examples 中所有 example.invalid 地址、令牌、实例和站点规则ID都是虚构夹具。

## Schema 之外仍必须校验

- Native 帧按 UTF-8 字节计数，最多262144字节；JSON嵌套不超过16层，重复键拒绝。
- 凭据材料按字节计数最多32768；Cookie作用域、分区语义、头换行和目标授权由运行期核验。
- u64十进制字符串需要再次检查数值溢出；字符长度约束不能替代数值检查。
- 同实例认证、session绑定、源可重放性证据、任务状态、revision、租约和幂等都由服务端执行，Schema通过不代表获准。
- 格式/容器兼容、资源身份、URL有效期、预计大小与精确大小须按媒体执行语义核验。
- TaskState 与 phase 的合法组合由状态机控制；响应还需与其原请求操作匹配。
- 包头 idempotency_key 对创建/交接绑定身份；其他操作用于关联或操作账本，不把全部op的不同payload当作创建冲突。操作去重按可信调用方、op、request_id。

## 握手与上下文

hello.stage 依次可为 negotiate、enroll（新实例）、challenge、prove。服务端返回的 challenge 是编码后的随机nonce。未证明持有登记凭据前只接受hello；首次登记只创建新实例。握手证明和登记秘密不进入普通日志/outbox；数据操作绑定当前已认证 Native 连接。

context.put建立作用域明确的上下文；context.renew携带原引用、预期上下文版本及当前材料续期，返回material_changed；context.revoke撤销。续期不能扩大站点授权或延长真实Cookie有效期。材料身份发生变化要新attempt和重新解析；只是同一材料延长本地租约不应打断正常长下载。撤销或到期会停止使用旧材料的外部工具树和对应relay连接。

## 本包验证与产品验证

资料生成时已验证正例、若干非法输入和契约元数据。真正实现后仍需Rust与扩展的序列化往返、IPC字节边界、状态/权限/崩溃测试；完整要求见ACCEPTANCE.md。示例中的日期和URL不是可以直接运行的线上任务。
