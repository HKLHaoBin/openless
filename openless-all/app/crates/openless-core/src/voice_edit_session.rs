//! Core state machine for issue #900's voice edit session.
//!
//! The host owns recording, focus and provider adapters. This module owns the
//! user-visible draft contract: an initial dictated segment is kept separate
//! from the field context, every edit turn is applied deterministically to the
//! preview, and only an explicit commit can produce replacement text.

use serde::{Deserialize, Serialize};

use crate::edit_plan::{apply_selection_edit_plan, EditApplyError, EditPlan};
use crate::types::SessionId;

const MAX_TURNS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSelection {
    /// UTF-8 character offsets, rather than byte offsets.
    pub start: u32,
    pub end: u32,
}

impl TextSelection {
    pub fn normalized(self) -> (usize, usize) {
        (
            self.start.min(self.end) as usize,
            self.start.max(self.end) as usize,
        )
    }

    pub fn is_non_empty(self) -> bool {
        self.start != self.end
    }

    fn validate(self, field_text: &str) -> Result<(), VoiceEditError> {
        let (start, end) = self.normalized();
        if start == end || end > field_text.chars().count() {
            return Err(VoiceEditError::InvalidSelection);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEditPhase {
    Dictating,
    DraftReady,
    Editing,
    Applying,
    Preview,
    Committing,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceEditTarget {
    /// Replace only the original selected range.
    Selection { start: u32, end: u32 },
    /// Replace the complete field when the session began at a caret.
    FullField,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditContext {
    pub field_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<TextSelection>,
    pub dictated_segment: String,
    pub draft: String,
    pub preview: String,
    pub turns: Vec<VoiceEditTurn>,
    /// Saved preview revisions. The current revision is `preview_history_index`.
    #[serde(default)]
    pub preview_history: Vec<String>,
    #[serde(default)]
    pub preview_history_index: usize,
    /// Monotonic token for the currently displayed preview revision.
    #[serde(default)]
    pub preview_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditTurn {
    pub instruction_raw: String,
    pub instruction_polished: String,
    pub edit_plan: EditPlan,
    pub preview_after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditSnapshot {
    pub session_id: SessionId,
    /// Host-owned identity of the current instruction, never a history index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_id: Option<SessionId>,
    pub phase: VoiceEditPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<VoiceEditContext>,
    pub target: VoiceEditTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditCommit {
    pub session_id: SessionId,
    pub text: String,
    pub target: VoiceEditTarget,
    pub turns: Vec<VoiceEditTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEditError {
    EmptyDraft,
    EmptyInstruction,
    InvalidSelection,
    InvalidPhase { phase: VoiceEditPhase },
    TooManyTurns,
    PreviewChanged,
    EditPlan(EditApplyError),
}

impl std::fmt::Display for VoiceEditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDraft => write!(formatter, "voice edit draft is empty"),
            Self::EmptyInstruction => write!(formatter, "voice edit instruction is empty"),
            Self::InvalidSelection => write!(formatter, "voice edit selection is invalid"),
            Self::InvalidPhase { phase } => write!(formatter, "voice edit is in phase {phase:?}"),
            Self::TooManyTurns => write!(formatter, "voice edit has too many turns"),
            Self::PreviewChanged => write!(formatter, "voice edit preview changed"),
            Self::EditPlan(error) => write!(formatter, "voice edit plan failed: {error}"),
        }
    }
}

impl std::error::Error for VoiceEditError {}

impl From<EditApplyError> for VoiceEditError {
    fn from(error: EditApplyError) -> Self {
        Self::EditPlan(error)
    }
}

#[derive(Clone)]
pub struct VoiceEditSession {
    session_id: SessionId,
    phase: VoiceEditPhase,
    context: Option<VoiceEditContext>,
    target: VoiceEditTarget,
}

impl VoiceEditSession {
    /// Start context capture before the initial recording. No draft is usable
    /// and no insertion can happen until [`Self::finish_dictation`] succeeds.
    pub fn start(
        field_text: impl Into<String>,
        selection: Option<TextSelection>,
    ) -> Result<Self, VoiceEditError> {
        let field_text = field_text.into();
        if let Some(selection) = selection {
            selection.validate(&field_text)?;
        }
        let target = selection
            .filter(|selection| selection.is_non_empty())
            .map(|selection| VoiceEditTarget::Selection {
                start: selection.start,
                end: selection.end,
            })
            .unwrap_or(VoiceEditTarget::FullField);
        Ok(Self {
            session_id: SessionId::new(),
            phase: VoiceEditPhase::Dictating,
            context: Some(VoiceEditContext {
                field_text,
                selection,
                dictated_segment: String::new(),
                draft: String::new(),
                preview: String::new(),
                turns: Vec::new(),
                preview_history: Vec::new(),
                preview_history_index: 0,
                preview_revision: 0,
            }),
            target,
        })
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn snapshot(&self) -> VoiceEditSnapshot {
        VoiceEditSnapshot {
            session_id: self.session_id,
            instruction_id: None,
            phase: self.phase,
            context: self.context.clone(),
            target: self.target,
        }
    }

    /// Finish the no-insert initial recording and construct the first preview.
    pub fn finish_dictation(
        &mut self,
        dictated_segment: impl Into<String>,
    ) -> Result<(), VoiceEditError> {
        self.require_phase(VoiceEditPhase::Dictating)?;
        let dictated_segment = dictated_segment.into();
        let context = self
            .context
            .as_mut()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        context.dictated_segment = dictated_segment;
        context.draft = merge_draft(
            &context.field_text,
            context.selection,
            &context.dictated_segment,
        )?;
        context.preview = context.draft.clone();
        context.preview_history = vec![context.preview.clone()];
        context.preview_history_index = 0;
        context.preview_revision = 0;
        self.phase = VoiceEditPhase::DraftReady;
        Ok(())
    }

    pub fn enter_editing(&mut self) -> Result<(), VoiceEditError> {
        if !matches!(
            self.phase,
            VoiceEditPhase::DraftReady | VoiceEditPhase::Preview
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.phase = VoiceEditPhase::Editing;
        Ok(())
    }

    /// Mark the model/application part of an instruction turn as in flight.
    /// The host calls this after the no-insert instruction recording has ended
    /// and before it awaits the provider, so the UI can distinguish recording
    /// from an edit plan being applied.
    pub fn begin_applying(&mut self) -> Result<(), VoiceEditError> {
        if self.phase != VoiceEditPhase::Editing {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.phase = VoiceEditPhase::Applying;
        Ok(())
    }

    /// Return an interrupted recording or provider turn to the ready draft.
    /// This keeps a transient instruction failure retryable without discarding
    /// the session or inserting anything into the target field.
    pub fn recover_instruction(&mut self) -> Result<(), VoiceEditError> {
        if !matches!(
            self.phase,
            VoiceEditPhase::Editing | VoiceEditPhase::Applying
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.restore_ready_phase()
    }

    /// Apply one already-polished instruction and its validated EditPlan.
    /// Keeping the model call outside this type makes the transition testable
    /// without mocking an internal provider.
    pub fn apply_instruction(
        &mut self,
        instruction_raw: impl Into<String>,
        instruction_polished: impl Into<String>,
        edit_plan: EditPlan,
    ) -> Result<(), VoiceEditError> {
        let expected_revision = self.preview_revision()?;
        self.apply_instruction_at_revision(
            expected_revision,
            instruction_raw,
            instruction_polished,
            edit_plan,
        )
    }

    pub fn apply_instruction_at_revision(
        &mut self,
        expected_revision: u64,
        instruction_raw: impl Into<String>,
        instruction_polished: impl Into<String>,
        edit_plan: EditPlan,
    ) -> Result<(), VoiceEditError> {
        if !matches!(
            self.phase,
            VoiceEditPhase::DraftReady
                | VoiceEditPhase::Editing
                | VoiceEditPhase::Applying
                | VoiceEditPhase::Preview
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        let raw = instruction_raw.into();
        let polished = instruction_polished.into();
        if raw.trim().is_empty() || polished.trim().is_empty() {
            return Err(VoiceEditError::EmptyInstruction);
        }
        let preview_before = self
            .context
            .as_ref()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?
            .preview
            .clone();
        if self.preview_revision()? != expected_revision {
            return Err(VoiceEditError::PreviewChanged);
        }
        if preview_before.trim().is_empty() {
            return Err(VoiceEditError::EmptyDraft);
        }
        let preview_after = apply_selection_edit_plan(&preview_before, &edit_plan)?;
        let context = self
            .context
            .as_mut()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        if preview_after == preview_before {
            self.phase = if context.turns.is_empty() {
                VoiceEditPhase::DraftReady
            } else {
                VoiceEditPhase::Preview
            };
            return Ok(());
        }
        if context.preview_history_index >= MAX_TURNS {
            return Err(VoiceEditError::TooManyTurns);
        }
        context.preview_history.truncate(context.preview_history_index + 1);
        context.turns.truncate(context.preview_history_index);
        context.preview_history.push(preview_after.clone());
        context.preview_history_index += 1;
        context.preview_revision = context.preview_revision.wrapping_add(1);
        context.preview = preview_after.clone();
        context.turns.push(VoiceEditTurn {
            instruction_raw: raw,
            instruction_polished: polished,
            edit_plan,
            preview_after,
        });
        self.phase = VoiceEditPhase::Preview;
        Ok(())
    }

    /// Move to an already-saved preview without invoking a model or host write.
    pub fn undo_preview(&mut self) -> Result<(), VoiceEditError> {
        self.require_preview_phase()?;
        let context = self
            .context
            .as_mut()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        if context.preview_history_index > 0 {
            context.preview_history_index -= 1;
            context.preview_revision = context.preview_revision.wrapping_add(1);
            context.preview = context.preview_history[context.preview_history_index].clone();
        }
        Ok(())
    }

    /// Move to an already-saved preview without invoking a model or host write.
    pub fn redo_preview(&mut self) -> Result<(), VoiceEditError> {
        self.require_preview_phase()?;
        let context = self
            .context
            .as_mut()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        if context.preview_history_index + 1 < context.preview_history.len() {
            context.preview_history_index += 1;
            context.preview_revision = context.preview_revision.wrapping_add(1);
            context.preview = context.preview_history[context.preview_history_index].clone();
        }
        Ok(())
    }

    pub fn preview_revision(&self) -> Result<u64, VoiceEditError> {
        self.context
            .as_ref()
            .map(|context| context.preview_revision)
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })
    }


    fn require_preview_phase(&self) -> Result<(), VoiceEditError> {
        if matches!(self.phase, VoiceEditPhase::DraftReady | VoiceEditPhase::Preview) {
            Ok(())
        } else {
            Err(VoiceEditError::InvalidPhase { phase: self.phase })
        }
    }

    /// Reserve the commit before the host writes the returned text externally.
    pub fn begin_commit(&mut self) -> Result<VoiceEditCommit, VoiceEditError> {
        self.begin_commit_at_revision(self.preview_revision()?)
    }

    pub fn begin_commit_at_revision(
        &mut self,
        expected_revision: u64,
    ) -> Result<VoiceEditCommit, VoiceEditError> {
        if self.preview_revision()? != expected_revision {
            return Err(VoiceEditError::PreviewChanged);
        }
        if !matches!(
            self.phase,
            VoiceEditPhase::DraftReady | VoiceEditPhase::Preview
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        let context = self
            .context
            .as_ref()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        if context.preview.trim().is_empty() {
            return Err(VoiceEditError::EmptyDraft);
        }
        self.phase = VoiceEditPhase::Committing;
        Ok(VoiceEditCommit {
            session_id: self.session_id,
            text: context.preview.clone(),
            target: self.target,
            turns: context
                .turns
                .iter()
                .take(context.preview_history_index)
                .cloned()
                .collect(),
        })
    }

    /// Finish only after the host has successfully written to the target field.
    pub fn complete_commit(&mut self) -> Result<(), VoiceEditError> {
        self.require_phase(VoiceEditPhase::Committing)?;
        self.phase = VoiceEditPhase::Completed;
        Ok(())
    }

    /// Keep the preview retryable when the host could not write to the target.
    pub fn recover_commit(&mut self) -> Result<(), VoiceEditError> {
        self.require_phase(VoiceEditPhase::Committing)?;
        self.restore_ready_phase()
    }

    pub fn cancel(&mut self) -> Result<(), VoiceEditError> {
        if matches!(
            self.phase,
            VoiceEditPhase::Committing | VoiceEditPhase::Completed | VoiceEditPhase::Cancelled
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.context = None;
        self.phase = VoiceEditPhase::Cancelled;
        Ok(())
    }

    fn restore_ready_phase(&mut self) -> Result<(), VoiceEditError> {
        let context = self
            .context
            .as_ref()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        self.phase = if context.turns.is_empty() {
            VoiceEditPhase::DraftReady
        } else {
            VoiceEditPhase::Preview
        };
        Ok(())
    }

    fn require_phase(&self, expected: VoiceEditPhase) -> Result<(), VoiceEditError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(VoiceEditError::InvalidPhase { phase: self.phase })
        }
    }
}

fn merge_draft(
    field_text: &str,
    selection: Option<TextSelection>,
    dictated_segment: &str,
) -> Result<String, VoiceEditError> {
    if let Some(selection) = selection.filter(|selection| selection.is_non_empty()) {
        selection.validate(field_text)?;
        let (start, end) = selection.normalized();
        return slice_chars(field_text, start, end).ok_or(VoiceEditError::InvalidSelection);
    }
    if !field_text.is_empty() {
        return Ok(format!("{field_text}{dictated_segment}"));
    }
    if dictated_segment.trim().is_empty() {
        return Err(VoiceEditError::EmptyDraft);
    }
    Ok(dictated_segment.to_string())
}

fn slice_chars(text: &str, start: usize, end: usize) -> Option<String> {
    let start_byte = if start == 0 {
        0
    } else {
        text.char_indices().nth(start)?.0
    };
    let end_byte = if end == text.chars().count() {
        text.len()
    } else {
        text.char_indices().nth(end)?.0
    };
    Some(text[start_byte..end_byte].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_draft_keeps_caret_text_and_voice_text_without_inventing_spacing() {
        assert_eq!(merge_draft("a", None, "b").unwrap(), "ab");
        assert_eq!(merge_draft("", None, "b").unwrap(), "b");
    }

    #[test]
    fn unicode_selection_uses_character_offsets() {
        assert_eq!(
            merge_draft("甲乙丙", Some(TextSelection { start: 1, end: 3 }), "").unwrap(),
            "乙丙"
        );
    }

    #[test]
    fn preview_history_undo_redo_and_divergence_only_switch_saved_previews() {
        use crate::edit_plan::{EditOperation, EditPlan};

        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        let replace = |find: &str, replace: &str| EditPlan {
            operations: vec![EditOperation::LiteralReplace {
                find: find.into(),
                replace: replace.into(),
            }],
            summary: None,
        };
        session.apply_instruction("one", "one", replace("original", "first")).unwrap();
        session.apply_instruction("two", "two", replace("first", "second")).unwrap();
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview, "second");
        session.undo_preview().unwrap();
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview, "first");
        session.redo_preview().unwrap();
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview, "second");
        session.undo_preview().unwrap();
        session.apply_instruction("branch", "branch", replace("first", "branch")).unwrap();
        session.redo_preview().unwrap();
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview, "branch");
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview_history.len(), 3);
    }

    #[test]
    fn no_op_edit_preserves_revision_and_redo_history() {
        use crate::edit_plan::{EditOperation, EditPlan};

        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        let replace = |find: &str, replace: &str| EditPlan {
            operations: vec![EditOperation::LiteralReplace {
                find: find.into(),
                replace: replace.into(),
            }],
            summary: None,
        };
        session.apply_instruction("one", "one", replace("original", "first")).unwrap();
        session.apply_instruction("two", "two", replace("first", "second")).unwrap();
        session.undo_preview().unwrap();
        let before = session.snapshot();
        session.apply_instruction("same", "same", replace("first", "first")).unwrap();
        let after = session.snapshot();
        assert_eq!(after.context, before.context);
        assert_eq!(after.phase, before.phase);
        session.redo_preview().unwrap();
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview, "second");
    }

    #[test]
    fn stale_revision_is_rejected_without_mutating_the_current_preview() {
        use crate::edit_plan::{EditOperation, EditPlan};

        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        let revision = session.preview_revision().unwrap();
        session.apply_instruction(
            "first",
            "first",
            EditPlan {
                operations: vec![EditOperation::FullRewrite { text: "first".into() }],
                summary: None,
            },
        ).unwrap();
        let result = session.apply_instruction_at_revision(
            revision,
            "stale",
            "stale",
            EditPlan {
                operations: vec![EditOperation::FullRewrite { text: "wrong".into() }],
                summary: None,
            },
        );
        assert_eq!(result, Err(VoiceEditError::PreviewChanged));
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview, "first");
    }

    #[test]
    fn no_op_edit_is_allowed_at_the_turn_limit() {
        use crate::edit_plan::{EditOperation, EditPlan};

        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        for index in 0..64 {
            session
                .apply_instruction(
                    format!("edit {index}"),
                    format!("edit {index}"),
                    EditPlan {
                        operations: vec![EditOperation::FullRewrite {
                            text: format!("v{index}"),
                        }],
                        summary: None,
                    },
                )
                .unwrap();
        }
        session
            .apply_instruction(
                "same",
                "same",
                EditPlan {
                    operations: vec![EditOperation::FullRewrite { text: "v63".into() }],
                    summary: None,
                },
            )
            .unwrap();
        assert_eq!(session.snapshot().context.unwrap().preview_history_index, 64);
    }

    #[test]
    fn stale_revision_after_divergent_branch_is_rejected() {
        use crate::edit_plan::{EditOperation, EditPlan};

        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        session.apply_instruction(
            "first",
            "first",
            EditPlan {
                operations: vec![EditOperation::FullRewrite { text: "first".into() }],
                summary: None,
            },
        ).unwrap();
        let stale_revision = session.preview_revision().unwrap();
        session.undo_preview().unwrap();
        session.apply_instruction(
            "branch",
            "branch",
            EditPlan {
                operations: vec![EditOperation::FullRewrite { text: "branch".into() }],
                summary: None,
            },
        ).unwrap();
        let result = session.apply_instruction_at_revision(
            stale_revision,
            "stale",
            "stale",
            EditPlan {
                operations: vec![EditOperation::FullRewrite { text: "wrong".into() }],
                summary: None,
            },
        );
        assert_eq!(result, Err(VoiceEditError::PreviewChanged));
        assert_eq!(session.snapshot().context.as_ref().unwrap().preview, "branch");
    }

    #[test]
    fn divergence_at_the_turn_limit_uses_the_active_prefix_budget() {
        use crate::edit_plan::{EditOperation, EditPlan};

        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        let rewrite = |text: String| EditPlan {
            operations: vec![EditOperation::FullRewrite { text }],
            summary: None,
        };
        for index in 0..MAX_TURNS {
            session.apply_instruction("edit", "edit", rewrite(format!("v{index}"))).unwrap();
        }
        let full = session.snapshot();
        assert_eq!(
            session.apply_instruction("overflow", "overflow", rewrite("overflow".into())),
            Err(VoiceEditError::TooManyTurns)
        );
        assert_eq!(session.snapshot(), full);
        session.undo_preview().unwrap();
        session.apply_instruction("branch", "branch", rewrite("branch".into())).unwrap();
        let branch = session.snapshot();
        let context = branch.context.as_ref().unwrap();
        assert_eq!(context.preview, "branch");
        assert_eq!(context.turns.len(), MAX_TURNS);
        assert_eq!(context.preview_history.len(), MAX_TURNS + 1);
        assert_eq!(context.preview_history_index, MAX_TURNS);
        session.redo_preview().unwrap();
        assert_eq!(session.snapshot(), branch);
    }

    #[test]
    fn interrupted_instruction_recovers_without_changing_preview_or_redo() {
        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        let before = session.snapshot();
        session.enter_editing().unwrap();
        session.begin_applying().unwrap();
        session.recover_instruction().unwrap();
        assert_eq!(session.snapshot(), before);
    }

    #[test]
    fn commit_rejects_an_unseen_revision_without_reserving_or_writing() {
        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        let before = session.snapshot();
        assert_eq!(
            session.begin_commit_at_revision(1),
            Err(VoiceEditError::PreviewChanged)
        );
        assert_eq!(session.snapshot(), before);
        assert_eq!(session.begin_commit_at_revision(0).unwrap().text, "original");
    }

    #[test]
    fn commit_uses_the_selected_preview_revision() {
        use crate::edit_plan::{EditOperation, EditPlan};

        let mut session = VoiceEditSession::start("original", None).unwrap();
        session.finish_dictation("").unwrap();
        session.apply_instruction(
            "edit",
            "edit",
            EditPlan {
                operations: vec![EditOperation::FullRewrite { text: "edited".into() }],
                summary: None,
            },
        ).unwrap();
        session.undo_preview().unwrap();
        let wire = serde_json::to_value(session.snapshot()).unwrap();
        assert_eq!(wire["context"]["previewRevision"], 2);
        assert_eq!(wire["context"]["previewHistoryIndex"], 0);
        assert_eq!(wire["context"]["previewHistory"][1], "edited");
        let commit = session.begin_commit_at_revision(2).unwrap();
        assert_eq!(commit.text, "original");
        assert!(commit.turns.is_empty(), "redo instructions cannot leak into the committed path");
    }
}
