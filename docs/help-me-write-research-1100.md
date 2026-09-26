# Typeless「帮我写」跟进调研（#1100）

调研日期：2026-09-26。基线：仓库工作区 `F:/编程/openless` 当前检出。本文仅基于 **Typeless 官方文档**、**OpenLess 源码**与 **GitHub issue/PR** 等一手材料，供实现 #1100 的工程师使用。

---

## 1. 摘要 / 结论

OpenLess **已具备** Windows 划词语音编辑（Selection Voice：选区 + 口述指令 → 问句走 QA / 非问句走 EditPlan 编辑 → PreviewConfirm 或直接替换）以及独立 QA / 选区润色通路；**尚未具备** Typeless「Help me write」所需的 **无选区 + 肯定式写作指令 → 成稿插入光标处** 主路径。

核心差距不是「缺一个独立产品面」，而是三条硬约束叠在一起：

| 约束 | 现状证据 |
|------|----------|
| 会话入口强制非空选区 | Host：`begin_selection_voice_session` 在无选区时直接 `selectionVoiceNoSelection`（`openless-all/app/src-tauri/src/coordinator/selection_voice_session.rs`）；Core：`SelectionVoiceService::begin_session` 校验 `capture.text.trim().is_empty()` → `"selected text must not be empty"`（`selection_voice_service.rs`） |
| 意图空间只有 Question / Edit | `SelectionVoiceIntent` 二元枚举；启发式把非问句一律判为 `Edit`（`selection_voice_intent.rs`）——在空选区场景会把「帮我写邮件」误当成「编辑选区」且无法启动 |
| 编辑模型假设存在 `<draft>` | `voice_edit_system_prompt_*` 要求对草稿做 EditPlan ops（`prompts.rs`）；空 draft 上跑 `literal_replace` 无意义，需要 **compose / draft** 提示词与输出契约 |

**推荐实现策略（与 #1100 一致）：** 在现有选区助手 / Selection Voice 热键上扩展，而不是另起一套无关会话；规则优先的三路分流（有选区编辑 / 无选区问句 QA / 无选区肯定句「帮我写」）；成稿复用 PreviewConfirm + 现有 `inserter.insert`，但要单独处理「空 `source_text`」的目标校验与粘贴语义（见 §7 与 #1014）。

#900 仍是更广的「多轮语音编辑会话」提案；#1100 的 MVP 不必等 #900 全量落地，但可复用其 EditPlan / 信封安全思路，并把 compose 提示词与 edit 提示词明确分开。

---

## 2. Issue 诉求复述

来源：[Open-Less/openless#1100](https://github.com/Open-Less/openless/issues/1100)（OPEN）。

**产品对标：** Typeless Windows「Help me write（帮我写）」——光标在任意文本框、**不必先选中文字**；触发 Ask-anything 类入口；口述写作意图；再按主快捷键结束；直接在输入框生成可使用的成稿（邮件 / 群聊 / 帖子 / 一页说明等）。官方说明见 §3。

**与 OpenLess 现状对照（issue 原文表格）：**

| 场景 | 现状（issue 描述） | 期望 |
|------|-------------------|------|
| 已选中文字 + 编辑指令 | 选区语音编辑可用 | 保持 |
| 无选区 + 问句 | 更接近 QA | 走 QA |
| 无选区 + 肯定式写作指令 | 缺少对等能力 | 走「帮我写」，生成并插入 |

**建议意图分流（issue，可规则优先）：**

1. 选区为空 → 候选只有「帮我写」或 QA，不是选区编辑。
2. 口令含「帮我写」等强信号 → 直接「帮我写」。
3. 否则：问句 → QA；肯定句 / 写作意图 → 「帮我写」。

**Acceptance criteria（摘录）：**

- [ ] 无选区 + 肯定式写作指令 → 可插入成稿
- [ ] 「帮我写」等强信号优先「帮我写」，不误进选区编辑或纯听写
- [ ] 无选区 + 问句 → QA；有选区 + 编辑指令 → 现有选区语音编辑
- [ ] 复用插入 / PreviewConfirm；尽量不破坏剪贴板与 IME 焦点
- [ ] 设置 / 帮助文案可后续补文档 issue

相关动机 issue：[Open-Less/openless#900](https://github.com/Open-Less/openless/issues/900)（语音编辑会话，仍 OPEN；提出多轮口述草稿 + EditPlan，非本 issue 的最小「无选区成文」范围）。

---

## 3. Typeless「帮我写」产品事实（引官方文档）

### 3.1 Help me write 发布说明（主源）

来源：[Turn simple instructions into ready-to-use writing with Help me write](https://www.typeless.com/help/release-notes/windows/use-help-me-write-desktop)（标注日期 **September 22, 2026**；归属 Windows release notes）。

**产品定位：** 把简单指令变成可直接使用的回复、邮件、帖子或文档；用户说明写什么、给谁、有什么要求，产品负责生成正文。

**交互步骤（文档原文流程）：**

1. 点击任意想开始写作的文本框。
2. 按一次 **Ask anything** 快捷键（默认：macOS `Fn + Space`，Windows `Right Alt + Space`）。
3. 说出想写的内容。
4. 说完后按一次 **主 Typeless 快捷键**（默认：macOS `Fn`，Windows `Right Alt`）。
5. Typeless 把指令变成可直接使用的文字，**直接出现在该文本框中**。

**用户说法特征（文档示例均为肯定 / 指令式写作，非问句）：**

- `Write an email asking if Emma's free this Friday afternoon. Sign off with 'Will.'`
- `Write a message to Maya saying I can't go to dinner tonight...`
- `Write a group chat message thanking everyone...`
- `I want to post about training for my first half marathon...`
- `I need a one-page concept note for a weekly team demo...`

**与选区的关系：** 该页强调「click any text field」「turns … into ready-to-use writing **directly in the text field**」，**未要求先选中文字**；亦未描述 Preview 弹窗——成稿表现为 **直接落入输入框**。

**Windows 发布索引页：** [Typeless Windows app release notes](https://www.typeless.com/help/release-notes/windows)（索引壳页；本功能详情以专用 release note URL 为准）。

### 3.2 与 Ask anything / Dictate 的边界（相关官方 quickstart）

来源：[How to use Ask anything](https://www.typeless.com/help/quickstart/ask-anything)。

Ask anything 桌面能力拆成多条路径，与「帮我写」共用 **Ask anything 快捷键 + 主快捷键结束** 的键位范式，但输出行为不同：

| 路径 | 是否要求选区 | 结束键后结果 |
|------|--------------|--------------|
| Speak to edit selected text | 要选区 | **替换选区** |
| Speak to ask questions about selected text | 要选区 | **弹窗答案**，原选区不变 |
| Speak to get quick answers | 文档步骤未要求选区 | **弹窗卡片** |
| Help me write（§3.1） | 文档未要求选区 | **文本框内成稿** |

来源：[How to use Dictate](https://www.typeless.com/help/quickstart/dictate)。

Dictate 是另一条入口（默认主快捷键本身）：口述后「polished writing appears where you began dictating」。Help me write 明确走 **Ask anything 入口**，语义是「按写作意图生成成稿」，不是普通听写润色；OpenLess 实现时也不应把「帮我写」 silently 降级成纯 dictation（#1100 AC）。

### 3.3 对 OpenLess 映射时应注意的产品事实

1. **入口复用 Ask 类快捷键**，结束用主听写键——OpenLess 对应更接近「选区助手 / Selection Voice」+ 全局录音结束语义，而非新建第三套热键（#1100 也写「不必 1:1 复制键位」）。
2. **无选区是一等公民**，不是失败分支。
3. **输出是字段内成稿**；OpenLess 可用 PreviewConfirm 作为安全层，但最终仍须能插入目标框。
4. 用户话术以 **英文肯定句 / 「Write…」「I want…」「I need…」** 为主；中文侧 #1100 额外要求「帮我写」强信号。

---

## 4. OpenLess 现状能力地图（引代码 / #900）

### 4.1 相关产品能力一览

| 能力 | 入口 / 开关 | 选区要求 | 意图 | 输出 | 平台备注 |
|------|-------------|----------|------|------|----------|
| 选区润色 | `selection_polish_hotkey`；关语音编辑时 | 非空 | 无（直接润色） | `SelectionPolishOutputMode::{DirectReplace, PreviewConfirm}` | 桌面；设置文案见 `zh-CN` `settings.selectionWorkspace` |
| **选区语音编辑（Selection Voice）** | 同一选区助手热键 + `selection_voice_enabled` | **强制非空** | Question / Edit | QA 面板 或 EditPlan → 预览/替换 | **Windows 优先**（`hotkey_loops.rs` 仅 `target_os = "windows"` 走 voice 分支；#995） |
| 划词 QA | `qa_hotkey` | 可空（空则纯问题） | 问答 | `QaPanel` | 桌面 + Android overlay（另有 #1088 互斥问题） |
| 听写 Dictation | 全局听写热键 | 不需要选区 | 无 | 润色后插入光标 | 全平台主路径 |
| #900 Voice Edit Session | 提案中 | 选区或全文草稿 | 多轮 EditPlan | 确认后 replace | **issue 仍 OPEN**；EditPlan 形态已被 Selection Voice 部分落地 |

架构索引：`docs/architecture.md` 将 `selection_voice_service` + `selection_voice_intent`、`qa_service`、`selection_service`、`edit_plan` 列为交互域；前端窗口含 `selection-voice-intent`、`selection-polish-preview`、`qa`（见同文档与 `docs/2.0-desktop-acceptance.md`「划词与 QA」行）。

### 4.2 Selection Voice：会话与硬门槛

**Host 启动（Windows）：**

- 热键：`handle_selection_workspace_hotkey_pressed` 在 `selection_voice_enabled` 时调用 `handle_selection_voice_pressed`（`coordinator/hotkey_loops.rs`）。
- `begin_selection_voice_session`：`resolve_selection_workspace_capture_with_diag()` → 无选区则 `Err("selectionVoiceNoSelection")`；无插入目标则 `selectionVoiceTargetUnavailable`（`coordinator/selection_voice_session.rs`）。
- Core `begin`：再次拒绝空 `capture.text`（`selection_voice_service.rs:begin_session`）。

历史修复：多次 commit「auto-hide capsule when begin fails without selection」说明 **无选区失败是已知产品行为**，不是偶然 bug。

**录音 / 结束：** `dispatch_hotkey_edge` 复用全局 `HotkeyMode`（Toggle / Hold / DoubleClick）（`selection_voice_service.rs`）；与 Typeless「Ask anything 开始 + 主键结束」不完全相同，但同属「第二入口录音 + 结束边」。

**Disposition / Route：**

- `SelectionVoiceDisposition::{AwaitingIntent, Question, Edit}`（`domains.rs`）
- `SelectionVoiceRoute::{AwaitingIntent, QuestionCompleted, EditConversationOpened, ReadyToApply}`（`domains.rs`）
- 无 `Compose` / `HelpMeWrite` 变体。

### 4.3 意图路由（已有，但只有两路）

模块：`openless-all/app/crates/openless-core/src/selection_voice_intent.rs`。

| 符号 | 作用 |
|------|------|
| `SelectionVoiceIntent::{Question, Edit}` | 仅两档 |
| `looks_like_question_instruction` / `BUILTIN_QUESTION_CUES` | `？` /「吗」「什么」「how」「what」等 |
| `resolve_selection_voice_intent_heuristic` | 自定义 `selection_voice_edit_keywords`（实为问句线索）命中 → Question；否则问句启发 → Question；**否则 Edit** |
| `classify_selection_voice_intent_with_provider_result` | Prompt / Manual / Heuristic / Auto（LLM XML/JSON + 启发回退） |
| `selection_voice_intent_classification_prompt`（`prompts.rs`） | 模型只输出 `<intent>edit\|question</intent>`；**非问句一律 edit** |

前端意图面板：`SelectionVoiceIntentPicker.tsx` 仅「提问」/「编辑选区」两按钮；文案 `selectionVoiceIntent.*`（`zh-CN.ts`）。

**对 #1100 的直接含义：** 「帮我写一封邮件」在现有启发下会落成 **Edit**；但空选区又无法 `begin` → 功能空洞。即使放开空选区，空 `<draft>` 上跑 EditPlan 仍错误。

### 4.4 编辑路径：EditPlan + 风格包 + PreviewConfirm

**提示词：** `voice_edit_system_prompt_xml` / `_json`、`voice_edit_user_prompt`、`voice_edit_injection_defense`（`prompts.rs`）——任务是「如何**修改**草稿」，输出 EditPlan ops；`full_rewrite` 仅作兜底 op。

**生成：** `SelectionVoiceService::generate_edit_plan` 组装 `<field_context/><draft/><instruction/>`，system prompt 经 `resolve_voice_edit_system_prompt(custom, pack VoiceEdit, format)`（`selection_voice_service.rs`）。风格包字段：`StylePack.voice_edit_prompt` / `StylePromptKind::VoiceEdit`（`style_packs.rs`）。

**输出模式：** 非 `DirectReplace` 时 `OpenConversation`（进 QA 面板预览）；`DirectReplace` 则 `ReadyToApply`（`selection_voice_service.rs` 约 1080–1110 行）。偏好字段复用 `selection_polish_output_mode`（设置里「结果处理」）。

**应用插入：** `confirm_selection_voice_preview` → `apply_selection_voice_preview_ticket`：`reactivate_selection_insertion_target` + `validate_selection_insertion_target(..., ticket.source_text)` + `inserter.insert(replacement_text, restore_clipboard, paste_shortcut)`（`selection_voice_session.rs`）。结果映射 `InsertStatus::{Inserted, PasteSent, CopiedFallback}`。

选区润色 PreviewConfirm 平行路径：`selection_service.rs` 在 `PreviewConfirm` 时 `ShowSelectionPreview`（`SelectionPolishPreview.tsx`）。

### 4.5 QA 路径（已支持无选区问句）

- `compose_qa_user_content`：选区为空时 **只发 question 字符串**，不包 `<selected_text>`（`qa_service.rs`）。这与 #118 设计「没选区静默降级为纯语音问答」一致（issue 已 CLOSED，由 #119 落地）。
- Selection Voice 判为 Question 后可转入 QA（`SelectionVoiceRoute::QuestionCompleted`；合同测试见 `tests/qa_contract.rs` 等）。
- Android overlay QA 与 Dictation 互斥问题见 [#1088](https://github.com/Open-Less/openless/issues/1088)——若将来把「帮我写」接到 overlay，需一并设计会话门闩。

### 4.6 设置与文案现状

`SelectionWorkspaceSection.tsx`（仅桌面热键能力 + Windows 显示「语音编辑」开关）：

- 关语音编辑：快捷键 = 选区润色。
- 开语音编辑：同一快捷键 = 口述指令；可选自动意图、额外问句线索、EditPlan XML/JSON、自定义 EditPlan system prompt。

`zh-CN` 明确写：「选中文字后按同一快捷键…」「编辑选区」——**产品文案默认假设有选区**（`settings.selectionWorkspace.hint`）。

### 4.7 #900 与已落地 MVP 的关系

| | #900（仍 OPEN） | 已落地 Selection Voice（#987 → #995 等） | #1100 |
|--|-----------------|------------------------------------------|-------|
| 目标 | 多轮口述草稿 + 语音指令编辑 + 确认发送 | 划词后单轮/面板内后续编辑 | **无选区单轮成文** |
| 选区 | 可选；可读全文 | **必须非空** | **必须允许空** |
| 模型输出 | EditPlan | EditPlan（+ 翻译快路径） | **compose 成稿**（非改草稿） |
| 复用点 | 信封、会话门、replace 插入 | 意图启发、PreviewConfirm、insert | 意图扩展 + compose prompt + 空选区插入 |

相关已合并 PR（一手）：

- [#995](https://github.com/Open-Less/openless/pull/995)：Windows Selection Voice MVP，意图分流，QA 面板预览（Closes #987）。
- [#1025](https://github.com/Open-Less/openless/pull/1025)：Windows 剪贴板 sentinel 竞争导致划词失败。
- [#1077](https://github.com/Open-Less/openless/pull/1077) / [#1076](https://github.com/Open-Less/openless/pull/1076)：EditPlan 解析与可配置提示词。

相关开放问题：

- [#1014](https://github.com/Open-Less/openless/issues/1014)：PreviewConfirm 路径 Ctrl+C 重校验破坏选区 → 嵌套重复插入（对「空选区插入」同样警示校验策略）。
- [#1033](https://github.com/Open-Less/openless/issues/1033)：插入后 IME 状态偶发未还原。
- [#994](https://github.com/Open-Less/openless/issues/994)：纯文本插入模式诉求（落字格式）。

### 4.8 代码索引（实现时优先打开）

| 路径 | 关键符号 / 职责 |
|------|-----------------|
| `crates/openless-core/src/selection_voice_service.rs` | `begin_session` 空选区拒绝；`generate_edit_plan`；disposition → route |
| `crates/openless-core/src/selection_voice_intent.rs` | `SelectionVoiceIntent`；启发式 / Auto 分类 |
| `crates/openless-core/src/prompts.rs` | `voice_edit_*`；`selection_voice_intent_classification_prompt` |
| `crates/openless-core/src/domains.rs` | `SelectionVoiceDisposition` / `Route` / `ApplyTicket` |
| `crates/openless-core/src/qa_service.rs` | `compose_qa_user_content`（空选区 QA） |
| `crates/openless-core/src/selection_service.rs` | PreviewConfirm / DirectReplace 选区润色 |
| `crates/openless-core/src/edit_plan.rs` | EditPlan 解析 / ops（compose 是否复用 `full_rewrite` 需设计） |
| `src-tauri/src/coordinator/selection_voice_session.rs` | Host begin/end/apply；空选区错误码 |
| `src-tauri/src/coordinator/hotkey_loops.rs` | 选区助手热键 → voice vs polish |
| `src-tauri/src/selection.rs` | 选区捕获、插入目标校验 |
| `src/pages/SelectionVoiceIntentPicker.tsx` | 人工意图二选一 |
| `src/pages/SelectionPolishPreview.tsx` / `QaPanel.tsx` | 预览确认 UI |
| `src/pages/settings/SelectionWorkspaceSection.tsx` | 选区助手设置 |
| `crates/openless-core/tests/selection_voice_contract.rs` | 热键边与合同 |

仓库内 **未检索到** 用户可见的「帮我写」/ `Help me write` 实现字符串；compose 能力目前不存在。

---

## 5. 差距与分流设计

### 5.1 存在 vs #1100 需求

| #1100 需求 | OpenLess 现状 | 差距 |
|------------|---------------|------|
| 无选区启动 Ask 类语音入口 | Selection Voice / 选区润色均要求非空选区；QA 热键可无选区但只做问答 | 需「空选区仍可录音」的会话分支；插入目标仍须有效（焦点在可编辑框） |
| 肯定式写作 → 成稿插入 | 非问句 → Edit + EditPlan | 需 Compose 意图 + compose prompt（直接正文或受控 `full_rewrite`） |
| 「帮我写」强信号 | 无；非问句一律 Edit | 扩展启发 / Auto 分类标签 / 关键词表 |
| 无选区问句 → QA | QA 已支持空选区；Selection Voice 问句路径已接 QA | 空选区时须先允许 begin，再分到 QA |
| 有选区编辑保持 | 已有 | 回归保护；勿让 compose 规则抢走「把 X 改成 Y」 |
| PreviewConfirm / insert | 有；校验假设 `source_text` 为原选区 | 空 `source_text` 的 validate / reactivate 语义；#1014 风险 |
| 不破坏剪贴板 / IME | `restore_clipboard_after_paste` 等已有；#1033 未闭环 | 复用 insert 管线；避免多余 Ctrl+C 探测 |
| 文档 / 设置说明 | 文案假设「选中文字」 | AC 允许后续文档 issue |

### 5.2 推荐分流（规则优先，与 #1100 对齐）

```text
触发：选区助手热键（或未来显式「帮我写」入口）
  │
  ├─ capture.selection 非空
  │     └─ 现有 Selection Voice：Question | Edit（不变）
  │
  └─ capture.selection 为空
        ├─ 插入目标不可用 → 明确错误（勿静默听写）
        ├─ 强信号「帮我写」/「写一封」/「写一条」/ Write a … → Compose
        ├─ looks_like_question → QA（复用 compose_qa_user_content 空选区）
        └─ 肯定 / 写作意图 → Compose
```

**不要**在空选区路径调用现有 `generate_edit_plan(draft="", …)` 除非明确把输出契约改成「唯一 op = full_rewrite」且 prompt 改为 compose——更干净的做法是独立 `compose_system_prompt` + 直接正文（或单一 `full_rewrite`），避免模型乱造 `literal_replace`。

**与 Typeless 差异（可接受）：** Typeless 文档写直接落入文本框；OpenLess 可用 PreviewConfirm 降低误插入成本，但 DirectReplace 模式应对齐「直接落入」。

### 5.3 实现草图（模块级）

1. **放宽 begin 门槛（条件化）：**  
   - Host：空选区时若 `insertion_target` 有效，仍创建 session（`source_text = ""`）。  
   - Core：`begin_session` 允许空文本，但要求后续 disposition 不得走「编辑非空草稿」的旧假设。  
2. **扩展意图枚举：** `SelectionVoiceIntent::{Question, Edit, Compose}`（或并行 `HelpMeWrite`）；更新 Manual Prompt UI 三选一；Auto prompt 增加 compose 标签。  
3. **Compose 生成：** 新 prompt「根据 `<instruction>` 生成可直接粘贴的成稿；不要问答；不要 EditPlan」；可选 style pack `compose_prompt`（可后续）。  
4. **应用：** 复用 `SelectionVoiceApplyTicket`，但 `validate_selection_insertion_target` 在 `source_text` 为空时跳过「选区内容一致性」/避免 Ctrl+C 重读（对齐 #1014 思路：确认窗 HWND 校验即可）。插入语义 = **在光标处粘贴**（等价听写落字），而非替换选区。  
5. **热键：** MVP 继续挂在选区助手键；文档说明「无选区时变为帮我写 / QA」。不必强制复制 Typeless Right Alt+Space。  
6. **测试：** 合同测试覆盖：空选区 +「帮我写…」→ Compose；空选区 +「今天是什么日子？」→ QA；有选区 +「翻译成英文」→ Edit；有选区 +「什么意思」→ Question。

---

## 6. 实现建议（可复用路径、提示词、入口、验收映射）

### 6.1 优先复用

| 复用项 | 理由 |
|--------|------|
| Selection Voice 热键边 / 录音 / ASR / 指令润色 | 已打通 Windows MVP（#995） |
| `selection_voice_intent` 启发骨架 | 已有问句表与 Auto 回退；扩展第三类即可 |
| QA 空选区 `compose_qa_user_content` | 问句路径零成本对齐 #118 |
| PreviewConfirm + QA 面板预览 | 用户已熟悉；#995 已把编辑预览统一进 QA 面板 |
| `inserter.insert` + clipboard restore | 与听写 / 选区替换同一落字栈 |
| XML 信封 + `sanitize_for_xml_envelope` | 延续 #609 / voice_edit 注入防御 |

### 6.2 提示词差异（必须分开）

| | Edit（现有） | Compose（#1100 新增） |
|--|--------------|------------------------|
| 输入 | `<draft>` + `<instruction>` | 主要 `<instruction>`（可选空 `<field_context>`） |
| 任务 | 修改草稿 | **从零生成成稿** |
| 输出 | EditPlan XML/JSON | **纯正文**（推荐）或单一 full_rewrite |
| 风格 | `voice_edit_prompt` / `selection_voice_edit_system_prompt` | 新默认 compose prompt；勿复用「禁止散文」的 EditPlan user framing（`voice_edit_user_prompt`） |

中文强信号建议（实现可配置，默认内置）：`帮我写`、`写一封`、`写一条`、`写一个`、`起草`、`拟一封`；英文：`write an email`、`write a message`、`draft a`、`help me write`（与 Typeless 示例对齐）。

### 6.3 入口与热键建议（优先级）

1. **P0：** 扩展现有「选区助手」在 `selection_voice_enabled` 时的空选区行为（改动面最小，符合 #1100「基于现有入口演进」）。  
2. **P1：** IntentPicker / Auto 分类支持 Compose；设置 hint 改写为「有选区编辑 / 无选区写作或提问」。  
3. **P2：** 文档 / 独立说明 issue（AC 允许后置）。  
4. **P3：** macOS / Linux egui 对齐（当前 voice 入口 Windows 限定——见 #995 scope）；Android 需先处理 #1088 类会话互斥再谈 overlay「帮我写」。

### 6.4 Acceptance criteria 映射

| AC | 建议验证方式 |
|----|--------------|
| 无选区肯定句成稿 | Windows：记事本 / 邮件草稿空焦点 → 选区助手键 → 「帮我写一封短邮件…」→ PreviewConfirm 确认 → 字段出现正文 |
| 「帮我写」强信号 | 单元：`classify_*` / 新 compose 启发；勿进 EditPlan |
| 无选区问句 → QA | 「今天是什么日子？」→ QaPanel 答案，**不** insert 成稿 |
| 有选区编辑保持 | 「把牵引改成迁移」仍 Edit；回归 `selection_voice_contract` / 手动划词 |
| 插入 / PreviewConfirm / 剪贴板 IME | DirectReplace 与 PreviewConfirm 各测一遍；确认未引入 #1014 式 Ctrl+C 重读；观察 IME（#1033） |
| 文档设置 | 可开 follow-up issue；本 MVP 至少改 `settings.selectionWorkspace.hint` 一句 |

### 6.5 建议落地顺序（给实现者）

1. Core：放宽空选区 begin + `Compose` disposition + compose prompt + 合同测试（不接 UI）。  
2. Host：`begin_selection_voice_session` 空选区走新分支；apply 时空 `source_text` 校验策略。  
3. 意图：启发强信号 + Auto prompt 三分类；IntentPicker 第三按钮。  
4. 前端：PreviewConfirm / QA 面板展示 compose 预览（可复用现有编辑预览气泡，文案改为「插入」而非「替换选区」）。  
5. 设置文案 + 手动 Windows 验收清单。  
6. （可选）评估是否把 Typeless 式「Ask 键开始 / 主听写键结束」做成可选键位映射——非 MVP。

---

## 7. 风险与开放问题

| 风险 | 说明 | 缓解 |
|------|------|------|
| 误路由 | 「能帮我写吗？」含问句线索又含「帮我写」 | 强信号优先 Compose（#1100）；或 Ambiguous → IntentPicker |
| 「总结一下」无选区 | 现启发会当 Edit，空选区无法执行 | 空选区 + 依赖选区的动词 → 提示「请先选中」或降级 QA；不要空跑 EditPlan |
| 与纯听写混淆 | 用户可能期望 Dictate | 仅在选区助手 / Ask 类入口进 Compose；听写键保持 dictation（对齐 Typeless 双入口） |
| 空选区检测不可靠 | Windows 剪贴板 fallback 可能误读旧剪贴板为「有选区」（历史 #1024/#1025） | 复用 sentinel；空/脏捕获时偏向 Compose/QA 而非 Edit |
| PreviewConfirm 校验 | #1014：确认前 Ctrl+C 破坏选区导致重复插入 | Compose 路径默认跳过内容重读；仅校验窗口/焦点 |
| 空 `source_text` 的 replace 语义 | 现 apply 假设替换选区 | 明确「光标插入」；勿发送删除选区的按键序列 |
| IME / 剪贴板 | #1033、restore_clipboard 竞态 | 复用听写 insert 参数；回归时关注中文 IME |
| 平台范围 | Selection Voice 主要在 Windows | #1100 未要求全平台；PR 描述写清 scope |
| #900 范围膨胀 | 易做成完整多轮会话 | MVP 单轮 compose；多轮留给 #900 |
| 模型把 compose 写成问答 | prompt 约束「只输出成稿」+ 拒绝 Markdown 寒暄 | 输出清洗；过短/过像答案时预览警告 |

**开放问题（需维护者拍板）：**

1. Compose 默认 PreviewConfirm 还是跟随现有 `selection_polish_output_mode`？  
2. IntentPicker 在空选区 Auto 失败时，第三选项文案用「帮我写」还是「生成正文」？  
3. 是否允许读取 **输入框全文**（非选区）作为 compose 上下文（#900 的 `field_text`）？MVP 建议 **否**，先对齐 Typeless「仅指令成文」。  
4. macOS 是否同期做，还是 Windows-first 与 #995 一致？

---

## 8. 参考链接

### Typeless（官方）

- [Help me write（Windows release note）](https://www.typeless.com/help/release-notes/windows/use-help-me-write-desktop) — 2026-09-22  
- [Windows release notes 索引](https://www.typeless.com/help/release-notes/windows)  
- [Ask anything quickstart](https://www.typeless.com/help/quickstart/ask-anything)  
- [Dictate quickstart](https://www.typeless.com/help/quickstart/dictate)  

### OpenLess issues / PRs

- [#1100](https://github.com/Open-Less/openless/issues/1100) — 本需求  
- [#900](https://github.com/Open-Less/openless/issues/900) — 语音编辑会话（更广提案）  
- [#987](https://github.com/Open-Less/openless/issues/987) / [#995](https://github.com/Open-Less/openless/pull/995) — Selection Voice MVP  
- [#118](https://github.com/Open-Less/openless/issues/118) / [#119](https://github.com/Open-Less/openless/pull/119) — 划词 QA（含无选区降级）  
- [#1077](https://github.com/Open-Less/openless/pull/1077) — EditPlan 配置与解析  
- [#1025](https://github.com/Open-Less/openless/pull/1025) — 划词剪贴板 sentinel  
- [#1014](https://github.com/Open-Less/openless/issues/1014) — PreviewConfirm 重复插入  
- [#1088](https://github.com/Open-Less/openless/issues/1088) — Android QA / Dictation 互斥  
- [#1033](https://github.com/Open-Less/openless/issues/1033) — 插入后 IME  

### 仓内文档 / 代码

- `docs/architecture.md` — 交互域与窗口标签  
- `docs/2.0-desktop-acceptance.md` — 划词与 QA 验收入口  
- `openless-all/app/crates/openless-core/src/selection_voice_{service,intent}.rs`  
- `openless-all/app/crates/openless-core/src/prompts.rs` — `voice_edit_*`  
- `openless-all/app/src-tauri/src/coordinator/selection_voice_session.rs`  
- `openless-all/app/src/pages/{SelectionVoiceIntentPicker,SelectionPolishPreview,QaPanel,settings/SelectionWorkspaceSection}.tsx`  

---

## 调研局限（未能从一手源核实）

1. **Typeless 运行时内部路由**：官方文档未公开「Help me write vs Ask question vs edit selection」的分类算法或模型提示词；仅能从交互步骤与示例话术推断。未安装/抓包 Typeless 客户端验证键位与插入实现细节。  
2. **Windows release notes 索引页** 抓取结果几乎为空壳，功能细节以专用 Help me write URL 为准。  
3. **营销页** [typeless.com/ask-anything](https://www.typeless.com/ask-anything) 有产品叙述，但本报告以 help/quickstart 与 release-notes 为准；未把营销文案当作行为合同。  
4. **非 Windows Host** 上 Selection Voice 完整 UX 以源码 `cfg` 与 #995 描述为准；未做 macOS/Linux 真机复测。  
5. **#900 是否有未合并实验分支** 未全量检索所有 fork；以主仓 issue 状态 OPEN + 当前树无独立 `VoiceEditSession` 模块为准（EditPlan 已在 Selection Voice 中落地）。  
