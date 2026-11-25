# Issue #1460 诊断候选包说明

- 基线：上游 `beta`，`c9d40c384c8835f6f0a81e1bf0a0d0322d7b891a`
- 分支：`diagnose/issue-1460-windows-voice-crash`
- 目标：只增加 Windows 诊断包的安全埋点，不改变普通构建的录音行为。
- 诊断开关：`issue-1460-diagnostics` Cargo feature；仅通过 GitHub Actions 的 `Release (all platforms)` 手动构建启用。
- 日志位置：沿用 OpenLess 的“导出日志”功能。埋点以 `[DEBUG-ISSUE1460] {单行 NDJSON}` 写入现有应用日志，不创建第二份用户日志，不记录音频、转写文本、设备路径、URI、凭据或原始错误文本。

## 当前证据模型

附件日志在故障录音开始后停止，约 199 秒后出现下一次启动；没有 panic、异常码或调用栈。因此本候选包先确认“退出前最后完成的生命周期边界”，不把静态猜测当成根因。

| ID | 假设 | 可证伪预测 | 诊断后若成立的下一步 |
|---|---|---|---|
| H1 | Windows native 音频流或设备回调发生异常 | 日志出现 `P2`/`P3` 的 native stream 或 stream error 记录，随后无 `P4` 正常退出 | 针对 CPAL/WASAPI/设备驱动路径取得事件查看器与 dump，并做最小设备矩阵 |
| H2 | 音频处理 worker 断开或队列背压导致录音链路终止 | 出现 `audio_processing_delivery` 的 `worker_disconnected` 或 `queue_full` | 修复对应 worker/队列生命周期；不要仅隐藏错误 |
| H3 | 录音错误已经产生，但没有传播到核心会话清理 | 出现 recorder runtime error，但没有后续用户可见故障清理相关日志 | 检查 runtime error 到 `report_recording_fault` 的完整传播与资源释放 |
| H4 | 录音停止/线程 join 生命周期存在异常 | 有 `recorder_stop_requested`，但没有 `audio_thread_exit` 或 `recorder_join`，或 join 结果为 `panicked` | 针对停止、取消和线程退出边界做单一变量修复与回归 |

## 埋点边界

- `P1`：录音请求、音频线程创建、native 启动结果。
- `P2`：native stream 成功或启动失败。
- `P3`：CPAL stream error、处理 worker 断开/队列满、watchdog 静默、runtime error 进入核心。
- `P4`：录音停止请求、音频线程正常退出、recorder join 结果。

每条记录包含固定的 `session_id`、`attempt_id`、`change_id`、进程级 `run_id`、单调递增 `event_seq` 和有限安全字段。正常构建不启用该 feature，因此不会改变普通安装包日志行为。

## 使用方式

1. 在 Actions 中对该分支手动运行 `Release (all platforms)`。
2. `platform` 选择 `windows`。
3. 启用 `windows_voice_crash_diagnostics`。
4. 安装生成的 Windows 诊断包。
5. 复现语音输入退出问题。
6. 使用应用内“导出日志”上传完整日志；重点保留 `[DEBUG-ISSUE1460]` 行及其前后 30 秒普通日志。
7. 同时提供 Windows 事件查看器 `Application Error` / `Windows Error Reporting` 记录；埋点只能证明 OpenLess 自身已走到哪个边界，不能替代 OS 崩溃转储。

本候选包不是“根因已确认”的产品修复；它是针对 Issue #1460 的第一轮可发布诊断候选。