//! One session's assistant state and the single entry point per feature (ADR-009). The app glue
//! feeds it committed lines as they arrive and calls a method when the user asks. Everything is in
//! memory only: lines, pins and answers vanish when the session ends. Nothing here logs.
use crate::actions::{extract, ActionItem, ActionList};
use crate::engine::{AssistantEngine, EngineError, RunControl};
use crate::extras::{
    agenda_check, explain, my_commitments, next_meeting_prep, parking_lot, say_it_short,
    AgendaStatus, Explained, ParkedItem, Pin, Pins, Prep, Shortened,
};
use crate::features::{Feature, FeatureSet};
use crate::prompt::PromptFormat;
use crate::qa::{answer, Answer};
use crate::recap::{recap, Recap};
use crate::title::suggest_title;
use crate::view::{Line, Note};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkspaceError {
    /// The feature is off in Settings or by policy.
    #[error("feature is off")]
    Disabled,
    #[error(transparent)]
    Engine(#[from] EngineError),
}

#[derive(Debug, Default)]
pub struct Workspace {
    lines: Vec<Line>,
    notes: Vec<Note>,
    participants: Vec<String>,
    pins: Pins,
}

fn need(features: FeatureSet, f: Feature) -> Result<(), WorkspaceError> {
    if features.has(f) {
        Ok(())
    } else {
        Err(WorkspaceError::Disabled)
    }
}

impl Workspace {
    pub fn push_line(&mut self, line: Line) {
        self.lines.push(line);
    }
    pub fn push_note(&mut self, note: Note) {
        self.notes.push(note);
    }
    pub fn set_participants(&mut self, names: Vec<String>) {
        self.participants = names;
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    /// Time of the newest line: "now" for recap windows.
    pub fn now_ms(&self) -> u64 {
        self.lines.last().map_or(0, |l| l.t_ms)
    }
    /// Forget everything (session ended).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn title(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
    ) -> Result<Option<String>, WorkspaceError> {
        need(features, Feature::Title)?;
        suggest_title(engine, control, format, &self.lines, &self.notes).map_err(|e| match e {
            crate::title::TitleError::Engine(e) => WorkspaceError::Engine(e),
        })
    }

    pub fn recap(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
        minutes: u32,
    ) -> Result<Recap, WorkspaceError> {
        need(features, Feature::Recap)?;
        Ok(recap(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            &self.participants,
            self.now_ms(),
            minutes,
        )?)
    }

    pub fn actions(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
    ) -> Result<ActionList, WorkspaceError> {
        need(features, Feature::Actions)?;
        Ok(extract(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            &self.participants,
            0,
        )?)
    }

    pub fn ask(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
        question: &str,
    ) -> Result<Answer, WorkspaceError> {
        need(features, Feature::Ask)?;
        Ok(answer(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            &self.participants,
            question,
        )?)
    }

    pub fn agenda(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
        items: &[String],
    ) -> Result<Vec<AgendaStatus>, WorkspaceError> {
        need(features, Feature::Agenda)?;
        Ok(agenda_check(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            items,
        )?)
    }

    pub fn commitments(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
    ) -> Result<Vec<ActionItem>, WorkspaceError> {
        need(features, Feature::Commitments)?;
        Ok(my_commitments(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            &self.participants,
        )?)
    }

    pub fn explain(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
        line_id: u64,
    ) -> Result<Explained, WorkspaceError> {
        need(features, Feature::Explain)?;
        Ok(explain(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            &self.participants,
            line_id,
        )?)
    }

    pub fn shorten(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
        from_id: u64,
        to_id: u64,
    ) -> Result<Shortened, WorkspaceError> {
        need(features, Feature::Shorten)?;
        Ok(say_it_short(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            &self.participants,
            from_id,
            to_id,
        )?)
    }

    pub fn prep(
        &self,
        features: FeatureSet,
        engine: &mut dyn AssistantEngine,
        control: &dyn RunControl,
        format: PromptFormat,
    ) -> Result<Prep, WorkspaceError> {
        need(features, Feature::Prep)?;
        Ok(next_meeting_prep(
            engine,
            control,
            format,
            &self.lines,
            &self.notes,
            &self.participants,
            &self.pins,
        )?)
    }

    // No model needed below.

    /// Pins the newest line (the moment the user is looking at now).
    pub fn pin_now(
        &mut self,
        features: FeatureSet,
        label: &str,
    ) -> Result<Option<Pin>, WorkspaceError> {
        need(features, Feature::Pins)?;
        let now = self.now_ms();
        Ok(self.pins.add(&self.lines, now, label))
    }

    pub fn pin_at(
        &mut self,
        features: FeatureSet,
        t_ms: u64,
        label: &str,
    ) -> Result<Option<Pin>, WorkspaceError> {
        need(features, Feature::Pins)?;
        Ok(self.pins.add(&self.lines, t_ms, label))
    }

    pub fn unpin(&mut self, features: FeatureSet, line_id: u64) -> Result<bool, WorkspaceError> {
        need(features, Feature::Pins)?;
        Ok(self.pins.remove(line_id))
    }

    pub fn pins(&self, features: FeatureSet) -> Result<Vec<Pin>, WorkspaceError> {
        need(features, Feature::Pins)?;
        Ok(self.pins.list().to_vec())
    }

    pub fn parking(&self, features: FeatureSet) -> Result<Vec<ParkedItem>, WorkspaceError> {
        need(features, Feature::ParkingLot)?;
        Ok(parking_lot(&self.lines))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FakeEngine, SharedControl};

    fn ws() -> Workspace {
        let mut w = Workspace::default();
        for (i, (who, text)) in [
            ("Others", "Welcome, today is the budget review."),
            ("Others", "The invoices are due on the fifteenth."),
            ("Me", "I will send them by Thursday."),
            ("Others", "Let's take the hiring plan offline."),
            ("Others", "The budget review covers laptops and invoices."),
            ("Others", "Next, we look at the hiring plan and the audit."),
        ]
        .iter()
        .enumerate()
        {
            w.push_line(Line {
                id: i as u64 + 1,
                t_ms: (i as u64 + 1) * 10_000,
                speaker: (*who).into(),
                text: (*text).into(),
            });
        }
        w
    }

    #[test]
    fn off_features_refuse_without_touching_the_engine() {
        let w = ws();
        let mut e = FakeEngine::with_replies(&["x"]);
        let c = SharedControl::default();
        let none = FeatureSet::none();
        assert_eq!(
            w.recap(none, &mut e, &c, PromptFormat::ChatMl, 5)
                .unwrap_err(),
            WorkspaceError::Disabled
        );
        assert_eq!(w.pins(none).unwrap_err(), WorkspaceError::Disabled);
        assert!(e.seen_prompts.is_empty());
    }

    #[test]
    fn extras_are_off_by_default_and_work_when_enabled() {
        let mut w = ws();
        let core = FeatureSet::default();
        assert_eq!(w.parking(core).unwrap_err(), WorkspaceError::Disabled);
        let on = core
            .with(Feature::ParkingLot, true)
            .with(Feature::Pins, true);
        assert_eq!(w.parking(on).unwrap().len(), 1);
        let p = w.pin_now(on, "hiring").unwrap().unwrap();
        assert_eq!(p.line_id, 6);
        assert_eq!(w.pins(on).unwrap().len(), 1);
        assert!(w.unpin(on, 6).unwrap());
    }

    #[test]
    fn ask_and_title_run_through_the_workspace() {
        let w = ws();
        let c = SharedControl::default();
        let mut e = FakeEngine::with_replies(&["The invoices are due on the fifteenth. [L2]"]);
        let a = w
            .ask(
                FeatureSet::default(),
                &mut e,
                &c,
                PromptFormat::ChatMl,
                "When are the invoices due?",
            )
            .unwrap();
        assert!(matches!(a, Answer::Found { .. }));
        let mut e = FakeEngine::with_replies(&["Budget review"]);
        let t = w
            .title(FeatureSet::default(), &mut e, &c, PromptFormat::ChatMl)
            .unwrap();
        assert_eq!(t.as_deref(), Some("Budget review"));
    }

    #[test]
    fn clear_forgets_the_session() {
        let mut w = ws();
        w.clear();
        assert_eq!(w.line_count(), 0);
        assert_eq!(w.now_ms(), 0);
    }
}
