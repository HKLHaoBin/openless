# Issue 1320：Android 风格 ZIP 导入取证

本分支保留可选诊断，并整合经过后续取证验证的 Android 风格 ZIP 文档导出修复。上游 issue 为 [#1320](https://github.com/Open-Less/openless/issues/1320)。固定起点为上游 `beta` 的 `2b89ecdd727f842443bded33d8dfcf1a21efe598`，不使用 fork 的同名 Beta 作为基线。

## 后续修复整合

- 将 `fix/issue-1320-style-zip-result` 的 `4475b1ae186f46d70ce5e93aec09bb7e4a2b257d` 合入本分支，而不是合入上游 beta。该来源的最终净改动只有四个核心修复/测试文件，不重新引入其已移除的另一套探针。
- 修复 Android 导出：文档 URI 使用 Core 字节编码后调用 ContentResolver；普通路径不变，编码/写入失败向上返回，不退回文件系统、不放宽 ZIP 校验或权限。
- 后续原设备日志 `openless-2026-10-31T17-07-31.log.txt` L68–85 已记录导出 5696 字节、重新读取同等字节、Core 成功、列表及目标行出现。它支持修复有效性，不证明最初空文件的来源，也不表示旧空 ZIP 可恢复。
- 新诊断包继续使用本分支的一份 `issue-1320-style-zip.ndjson`。P4 同时观察 ZIP 导出编码、写入和终态，以及导入实际读取字节数；仍为 P1/P3/P4/P5 四个边界，不新增定时器或原生回调覆盖。
- 只用既有 **实验与扩展 → 调试工具 → 导出错误日志**。在导出副本内追加当前覆盖快照及专用 NDJSON，不修改普通源日志，不新增风格页导出按钮；失败/待送达计数来自当前 WebView producer，重载之前仍未知。
- 此整合提交须重新独立审查、仅推 fork，重新执行 exact-head Actions；旧提交的全绿结果不代替此提交验证。验收通过后在 issue 交付这份带诊断的 Debug 测试包，不发布正式 Release。

## 范围与现有证据

- 症状：荣耀 Android 环境「风格包 → 导入 ZIP」没有可见反应。
- 用户 APK 版本、硬件型号、Android API、WebView 版本未知。「荣耀 Magic 11」按原始描述保留，不擅自确认为某硬件型号。
- 原日志 `openless-2026-10-31T11-40-09.log.txt` 只读保存，包含语音转写，不随代码或产物公开。
- 原日志 L130 收到一次导出命令及 `content://` 类型目标；L131–132、L139–140 可看到创建模板与预览。它们不是 ZIP 导入成功证据。
- 全文未找到导入入口日志。但未确认用户 APK 的探针覆盖与日志完整性，不能推断入口从未执行。
- 现有源码已有 Android dialog 初始化、capability、受限 content URI reader。不能无证据扩大存储权限或把 URI 当路径的问题重新套用。
- 短提示是静态风险；延长 toast 不等于修复原始导入问题。

## 正常行为模型

| 契约 | 来源 | 可见结果与因果顺序 | 例外与观察边界 | 现有证据分类 |
| --- | --- | --- | --- | --- |
| N1 | 用户预期；Style.tsx 的导入处理 | 一次点击进入导入态，文件选择器出现，或出现可读错误 | 用户取消合法；没有人为时间预算，pending 只能记为尚未完成 | 用户报告结果不符；具体失败边界 `INSUFFICIENT_EVIDENCE` |
| N2 | 导入 IPC 与随后 loadPacks | 选定合法 ZIP → 受限读取/验证/持久化 → 刷新列表并选中本次包 | 归档校验失败不得写入；读取/解析成功不等于列表呈现 | `INSUFFICIENT_EVIDENCE` |
| N3 | handler 的 finally 与 busy 状态、错误反馈 | 成功、取消、异常均终止 busy；失败对用户可读 | 未结算 Promise 不应被虚构成超时；同一错误的重复操作单独观察 | 反馈短暂为 `SUSPICIOUS`，非真机因果确认 |

单次导入的中间事件不可被用作其他导入的成功证据。跨层墙钟/写入顺序不构成完整因果序；仅使用真实回调、操作标签和相应覆盖说明。计数/性能预算对本问题没有已知外部要求，不添加任意阈值来判定产品失败。

## 可证伪假设

| ID | Hypothesis | Falsifiable prediction | If true, what change would make the bug disappear? | Probe IDs |
| --- | --- | --- | --- | --- |
| H1 | 选择器 Intent/MIME 启动边界失败（N1） | JS 发起打开后返回有界的 activity_not_found 等类别；后续受控 MIME 对照区分该变量 | 只修已被证实的 Intent/MIME 分支，保留所有归档校验 | P1/P3 |
| H2 | Activity 结果/桥接 Promise 未结算（N1/N3） | JS 出现打开事件却无终态，仅能缩小范围；确认丢失原生回调仍需额外原生证据 | 只修已证实的回调或生命周期边界，不增加定时 no-op、sleep 或盲重试 | P1/P3 |
| H3 | 操作已返回错误而反馈未被用户看到（N1/N2/N3） | 有有界错误类别、busy 终态，但提示呈现/可读性不符；后端成功也须核查列表结果 | 针对真实失败做一变量修复，反馈可读性另按原始结果契约验证 | P1/P3/P4/P5 |

上述三项是首次调研时的假设，首次均为 `INCONCLUSIVE`，不据此猜测行为修复；后续证据及已整合的修复见文首。实际实现四个观测边界：P1 点击/结束/卸载、P3 JS 插件加载与 picker Promise、P4 后端 ZIP 编码/读取/写入/归档、P5 列表应用及 DOM/toast commit。未实现 P2，不观测原生 Activity 或插件内部回调，覆盖快照明确标注。因此 pending 只能缩小范围，不能单凭缺失事件确认 H2。一个边界可发出多个有限事件，不允许无限加探针。

## 取证安全与交付门禁

- 单一专用应用私有日志：`files/OpenLess/issue-1320-style-zip.ndjson`。NDJSON 使用 `diagnosing-bugs.v1`，仅追加；不清空、轮换或删除已有应用日志。
- 仅诊断分支手动构建启用；正常构建默认关闭。没有定时器、轮询、超时兜底、权限放宽或取消语义更改。
- 日志只允许枚举、布尔值、有限计数/大小、单调时长、运行内生成的操作标记。不得包含 URI、文件名、归档内容、pack ID、语音/提示词或原始异常文本。
- UI 反馈只在当前操作与反馈所有者的 run/operation 均匹配时关联。前端送达计数明确按随机页面实例标记，快照声明先前页面实例覆盖未知；重建页面后出现零计数不代表整个后端 run 从未丢失事件。
- 导出必须先成功追加当前覆盖记录；若日志已达上限、写入失败或尾行不完整，拒绝将旧快照当成成功取证结果导出，原文件仍原样保留。
- 探针不是零开销：序列化、IPC、同步追加都会增加观察成本。原生 API 边界的观测不等于插件内部回调必定完成。丢失、未就绪、日志上限或导出快照尚未排空都必须作为覆盖限制，而非成功。
- 所有编译、依赖安装、自动化测试和打包仅在 GitHub Actions 执行，本地不执行这些步骤。
- 最后候选必须经独立 `review-diff-bugs` 只读审查，修复审查发现后重审，明确 `no bug found` 后才提交推送 fork。
- 不创建 PR、tag 或 Release。诊断构建成功只代表可供取证，不代表原始症状已修复。

## 分支构建范围

1. `CI`：手动选 `platform=all`；保留 Windows/macOS/MSRV/Android/Harmony/Linux/浏览器等原有门禁，并检查诊断功能开启路径。
2. `Release Tauri (cross-platform)`：手动选 `platform=all`，`release_tag` 留空；Windows x64、macOS arm64/x64 工作流产物，不发布。
3. `Android APK (debug)`：原反馈 Android 设备先构建 `abis=aarch64`，`style_zip_diagnostics=true`；要求签名诊断包时 `signed_debug=true`。不能根据仓库有签名配置便承诺与用户安装包证书一致；覆盖安装若不兼容，不要求卸载丢数据，应先确认签名与备份策略。
4. Linux 包由全量 CI 内的可复用构建门禁产生并核实 artifact。不调用要求 release tag 的独立发布流程。
5. 本问题不涉及原生 iOS 或 Harmony 分发，不触发商店上传、签名发布或无关 release 工作流。

每次推送先核实本地/fork SHA 一致，按 workflow、branch、event、head SHA 解析 run。每个 run/attempt 只调用一次 `gh run watch --exit-status`；失败保存完整日志，按责任范围修复、重审、推送后重新触发。最终核实所有要求 job、非空未过期产物、APK checksum 与来源 SHA。

## 后续修复闭环

诊断包通过后，需用户在原设备复现并返回专用诊断日志与安全可见结果。先按 N1–N3 分类，再为 H1–H3 分别标记 `CONFIRMED` / `REJECTED` / `INCONCLUSIVE` 与修复资格。只选择一项 `ACTIONABLE` 假设做可逆修复，记录 `attempt_id` / `change_id`，复用探针与同一日志，构建后复测原触发。自动化测试和干净审查不能替代原设备结果。用户确认前不移除探针、不删除诊断日志。
