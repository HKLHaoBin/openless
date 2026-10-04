use openless_core::{
    EditOperation, EditPlan, TextSelection, VoiceEditError, VoiceEditPhase, VoiceEditSession,
    VoiceEditTarget,
};

fn literal(find: &str, replace: &str) -> EditPlan {
    EditPlan {
        operations: vec![EditOperation::LiteralReplace {
            find: find.into(),
            replace: replace.into(),
        }],
        summary: Some(format!("replace {find}")),
    }
}

#[test]
fn draft_uses_existing_field_and_appends_the_first_dictated_segment() {
    let mut session = VoiceEditSession::start("已有内容", None).unwrap();
    session.finish_dictation("，这是口述补充").unwrap();

    let snapshot = session.snapshot();
    let context = snapshot.context.as_ref().unwrap();
    assert_eq!(context.field_text, "已有内容");
    assert_eq!(context.dictated_segment, "，这是口述补充");
    assert_eq!(context.draft, "已有内容，这是口述补充");
    assert_eq!(context.preview, context.draft);
    assert_eq!(snapshot.phase, VoiceEditPhase::DraftReady);
    assert_eq!(snapshot.target, VoiceEditTarget::FullField);
}

#[test]
fn selected_context_is_the_edit_draft_but_keeps_the_full_field_attachment() {
    let selection = TextSelection { start: 2, end: 4 };
    let mut session = VoiceEditSession::start("甲乙丙丁", Some(selection)).unwrap();
    session.finish_dictation("口述不会覆盖选区上下文").unwrap();

    let context = session.snapshot().context.unwrap();
    assert_eq!(context.field_text, "甲乙丙丁");
    assert_eq!(context.selection, Some(selection));
    assert_eq!(context.draft, "丙丁");
    assert_eq!(
        session.snapshot().target,
        VoiceEditTarget::Selection { start: 2, end: 4 }
    );
}

#[test]
fn multiple_instruction_turns_are_applied_to_preview_and_commit_only_happens_once() {
    let mut session = VoiceEditSession::start("今天下雨", None).unwrap();
    session.finish_dictation("").unwrap();

    session.enter_editing().unwrap();
    session.begin_applying().unwrap();
    session
        .apply_instruction("改成晴天", "改成晴天", literal("下雨", "晴天"))
        .unwrap();
    session.enter_editing().unwrap();
    session.begin_applying().unwrap();
    session
        .apply_instruction("再正式一点", "再正式一点", literal("今天", "今日"))
        .unwrap();

    let snapshot = session.snapshot();
    assert_eq!(snapshot.phase, VoiceEditPhase::Preview);
    assert_eq!(snapshot.context.as_ref().unwrap().preview, "今日晴天");
    assert_eq!(snapshot.context.as_ref().unwrap().turns.len(), 2);
    assert_eq!(
        snapshot.context.as_ref().unwrap().turns[0].preview_after,
        "今天晴天"
    );

    let commit = session.commit().unwrap();
    assert_eq!(commit.text, "今日晴天");
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Completed);
    assert!(matches!(
        session.commit(),
        Err(VoiceEditError::InvalidPhase { .. })
    ));
}

#[test]
fn applying_phase_is_visible_and_recoverable_without_insertion() {
    let mut session = VoiceEditSession::start("草稿", None).unwrap();
    session.finish_dictation("").unwrap();
    session.enter_editing().unwrap();
    session.begin_applying().unwrap();
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Applying);
    session.recover_applying().unwrap();
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Editing);
    assert!(matches!(
        session.commit(),
        Err(VoiceEditError::InvalidPhase { .. })
    ));
}

#[test]
fn cancellation_clears_the_pending_commit_and_never_returns_text() {
    let mut session = VoiceEditSession::start("草稿", None).unwrap();
    session.finish_dictation("").unwrap();
    session.cancel().unwrap();

    assert_eq!(session.snapshot().phase, VoiceEditPhase::Cancelled);
    assert!(session.snapshot().context.is_none());
    assert!(matches!(
        session.commit(),
        Err(VoiceEditError::InvalidPhase { .. })
    ));
}
