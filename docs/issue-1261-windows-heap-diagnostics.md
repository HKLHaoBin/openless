# Issue #1261 Windows 堆损坏诊断构建

本分支是定位用构建，不是已确认修复版。Bugbot 无 bug、CI 通过和安装包构建成功不代表用户现场的 `c0000374` 已修复；目前没有原始 dump 或首次非法访问的因果证据。不提 PR，不发布 Release/tag，不预先封禁 WO Mic、MacType 或 TSF。

## 现场与正常合同

来源：[Issue 正文](https://github.com/Open-Less/openless/issues/1261)、[16:43 插入阶段退出补充](https://github.com/Open-Less/openless/issues/1261#issuecomment-6382195634)、[关联 #1227](https://github.com/Open-Less/openless/issues/1227)。

N0：正常听写不应导致原生进程静默退出；取消/明确失败可不插入，但重启不是原操作完成。N1：音频消费端接收 16 kHz mono i16 LE。N2：关闭接收、pause/drop、旧帧 drain、worker join 依次完成；drop 后排空旧帧合法。N3：窗口工作投递到主线程，投递成功不同于任务完成。N4：IME unknown 不代表未插入，不能据此自动重放。N5：warm/cold 交接应保持有效资源所有权。

现有材料仅支持 N0 的报告者现场违约；N1–N5 仍缺完整观测。首块 PCM 标记、缺失 drop 日志、ntdll 检测线程或模块加载列表，均不能单独证明原因。已匹配官方 Beta2 EXE/PDB，近期 RVA 指向 capsule mirror 同步、定位及 Wry Context 析构，但离散 RVA 不能拼成完整栈，也不等于首次写坏点。

## 候选假设

| ID | Hypothesis | Falsifiable prediction | If true, what change would make the bug disappear? | Probe IDs |
|---|---|---|---|---|
| H3 | mirror/Wry 生命周期失配（N3） | 完整栈或探针显示窗口资源操作失配；仅绕过 mirror 同步时同一失配消失 | 在后续有证据的诊断尝试中，仅绕过 mirror 同步；不同时改录音或插入 | P3 |
| H1 | 录音关闭或 warm/cold 交接失配（N1/N2/N5） | 关闭/join 后仍有相关生产活动，不只是合法旧帧 drain | 若来自 warm 交接，仅关闭 warm capture 后相应失配及故障消失 | P1/P2 |
| H5 | MacType 钩子参与窗口/堆破坏（N3） | 控制其他条件时，首次非法访问随该注入条件改变 | 仅对该进程排除 MacType；不能以模块加载或短时未复现证明 | P3 |
| H4 | 插入个案的 IME 终结交错（N4） | 有 submit/cleanup 失配，paste 不走该失配路径 | 仅切换插入方式；此假设不单独解释此前录音期退出 | P4 |
| H2 | 增强 DSP 状态失配（N1） | 先确认启用增强；关闭后可复核的处理异常与故障同时消失 | 仅关闭增强；若原配置未启用，则该具体假设不适用 | P2 |

全部假设仍为 INCONCLUSIVE，没有 ACTIONABLE 产品修复。首轮返回现场后先执行合同语义判读，再选一个有证据、可逆的单变量修复，不以增加通用日志替代修复尝试。

## 构建与身份

- 默认 feature 关闭。仅手动分支桌面构建传 `windows_heap_diagnostics=true` 时，Windows Tauri build 使用 `--features issue-1261-diagnostics`；tag、目标 Release 修复及其他平台不启用。
- `release-tauri.yml` 仍保留 Windows line tables/PDB。上传前静态校验 EXE/PDB GUID+Age；不匹配或损坏直接失败。`windows-symbols.json` 记录源码 SHA、run URL、两文件哈希及身份。
- Windows artifact 包含安装包、裸 EXE、PDB、原 provenance 和符号 manifest。安装包内 EXE 与裸 EXE 的哈希可能不同，分析 dump 时仍须核对实际模块 CodeView identity，不拿新构建符号解析旧 dump。
- 仅 Actions 执行编译与测试：默认路径、Windows opt-in production path、standalone writer 测试、原生 recorder 模块测试、符号解析合成样本及 workflow 合同。Windows 完整 Tauri `--no-run` 是编译验证，不是运行测试。

## 四个探针族与唯一日志

`%TEMP%\openless-issue-1261-9e0ffd34-session.ndjson`

- P1：capture 配置、关闭接收、pause/drop 进入/返回及 worker join；回调仅更新原子计数，不写盘。
- P2：增强初态、首块输出、Flush/Shutdown 汇总（字节、块数、奇数/空块）；旧帧可以在 drop 后输出。
- P3：主线程投递/闭包进入/返回、布局变化、mirror 数量、过期 epoch 跳过。source 1=一般投递、2=mirror 同步、3=既有 watcher。MacType presence 是投递边界的即时查询快照，不证明钩子有责。
- P4：Rust 可见的 IME 操作开始、ack、最终结果与查询次数；操作码 1=commit、2=update、3=finish、4=cancel。不能据此确认 DLL 内 COM 对象已释放；无 TSF 操作时缺少 P4 合法。

每行使用 `diagnosing-bugs.v1`，附 session/run/probe/hypothesis、run-local operation label、源端 event_seq、单调 duration_us 和 coverage 计数。safe 通过封闭 key/value API 仅允许数字/布尔，不接收语音、识别文本、设备名、真实 token/PID/TID/HWND、窗口标题、路径、凭据或任意错误文本。

四点不覆盖所有原生模块；P1/P2 共用 capture operation label，P3/P4 有各自操作 label，不能凭相邻时间线强行认定跨族因果。投递可在当前主线程立即执行，因此 TaskEntered 先于 DispatchReturned 不一定违约；Rust unwind 的退出记录不是成功。

诊断通过 512 条有界队列 try_send 交给单 writer，不让音频回调等磁盘。启用时仍有分配、原子操作、线程初始化和窗口事件序列化开销，可能改变时序；不加定时器、延迟或重试。全量窗口边界会持续增长日志，请仅用于定位，不做长期无界运行。不轮转、覆盖、删除本轮或用户原有日志。

队列满、I/O 错误和崩溃会丢失尾部：coverage.dropped/coverage.io_failed 只覆盖已观测状态，不能证明最后一条之后没有损失。日志未产生、末行截断或缺少终态时，应记为覆盖不足，不从缺失推断路径未执行。已接受记录由独立 writer 追加，进程退出不保证队列排空；不为此延迟产品退出。

## 后续现场步骤

优先在报告者本地或私密通道分析已有 16:16/16:43 dump，核对模块身份、异常上下文、完整线程栈及可用堆页；mini dump 可能缺少堆内存。不要上传公开 Issue。若仍不足，使用本诊断包。

**请你复现 bug**

1. 备份配置/历史，安装指定 Action/提交的 Windows 诊断构建（不能仅以仍可能相同的应用版本号辨认）。先确认该 run 的 `windows_heap_diagnostics=true`。
2. 保持原设备、云端 ASR、润色、插入模式及其他原环境，不同时切换变量。
3. 按平时方式开始录音并正常结束，观察到出字、明确失败或进程退出；无需额外反复取消，也不要求先启用 PageHeap。
4. 记录大致时间、录音中/停止/插入阶段和实际结果：正确、错误、无变化、停滞、重复或未知。
5. 私密返回上述唯一 NDJSON 日志。不要修改、清空或删除日志，不公开语音、凭据或内存转储；未产生文件直接说明。未复现请说明使用时长及覆盖路径，记为 INCONCLUSIVE，而不是已修复。
