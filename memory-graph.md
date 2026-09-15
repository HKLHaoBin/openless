# Memory Graph — openless

- slug: openless
- path: f:/编程/openless
- updated: 2026-09-15

## Summary

OpenLess：Tauri 2 + Rust + WebView 听写/选区助手。上游 Open-Less/openless，维护者克隆 HKLHaoBin/openless。

## Entities

- SelectionVoice (Feature): 选区语音问答/编辑，EditPlan 结构化改写
- EditPlan (Module): XML/JSON 操作计划，本地确定性 apply
- StylePack (Entity): 含 prompt / selectionPrompt / voiceEditPrompt
- Issue1076 (Issue): EditPlan 解析失败（模型输出正文）
- Issue1046 (Issue): 历史页对已完成转录的录音提供重新转录与试听

## Relations

- SelectionVoice --uses--> EditPlan: 编辑意图生成方案后替换选区
- SelectionVoice --reads--> StylePack.voiceEditPrompt: 空则 prefs 自定义，再则默认 XML/JSON prompt
- EditPlan --parses-with-priority--> Xml|Json: 用户选择优先格式，另一种兜底
- QA Panel --shows--> model_output: 解析失败时展示原始模型输出
- History --retranscribes--> archived_recording: 有归档 WAV 的传统 ASR 条目可用当前 provider 重转

## Facts

- 2026-09-14 开分支 fix/selection-voice-editplan-prompt-format（基于 upstream/beta）
- Issue: https://github.com/Open-Less/openless/issues/1076（完全解决前不提 PR）
- PR: https://github.com/Open-Less/openless/pull/1077（目标 beta，关联并关闭 #1076）
- 根因：听写润色 user framing「只输出正文」与 EditPlan system prompt 冲突
- folia-major 参考：OUTPUT CONTRACT + 剥围栏/平衡括号候选解析
- 2026-09-15 基于 upstream/beta 创建 fix/1046-history-retranscription；历史页将重新转录入口从失败状态扩展到所有有归档录音的传统 ASR 条目，继续保留多模态能力边界

## Decisions

- 用户可选 EditPlan 输出优先 XML 或 JSON；解析双向兜底
- 提示词：设置自定义 > 风格包 voiceEditPrompt > 内置默认
- 失败错误保留 ---model_output--- 供 QA 面板展示
- 重新转录按钮不改变录音归档隐私策略：只有 `hasAudioRecording` 为 true 且非多模态条目展示，成功/润色失败/转录失败均可使用
