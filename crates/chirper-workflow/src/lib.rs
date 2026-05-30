use chirper_core::{
    AsrEngine, AudioSource, ChirperError, ChirperResult, DictationMode, Formatter, InsertionTarget,
    TextInserter, Transcript, WorkflowEvent, WorkflowState,
};

#[derive(Debug)]
pub struct DictationWorkflow<A, S, F, I> {
    audio: A,
    asr: S,
    formatter: F,
    inserter: I,
    mode: DictationMode,
    state: WorkflowState,
}

impl<A, S, F, I> DictationWorkflow<A, S, F, I>
where
    A: AudioSource,
    S: AsrEngine,
    F: Formatter,
    I: TextInserter,
{
    pub fn new(audio: A, asr: S, formatter: F, inserter: I) -> Self {
        Self {
            audio,
            asr,
            formatter,
            inserter,
            mode: DictationMode::default(),
            state: WorkflowState::default(),
        }
    }

    pub fn state(&self) -> WorkflowState {
        self.state
    }

    pub fn mode(&self) -> DictationMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: DictationMode) {
        self.mode = mode;
    }

    pub fn start_recording(&mut self) -> ChirperResult<Vec<WorkflowEvent>> {
        if self.state != WorkflowState::Idle {
            return Err(ChirperError::Unsupported(format!(
                "cannot start recording while state is {:?}",
                self.state
            )));
        }

        self.audio.start_recording()?;
        Ok(self.transition(WorkflowState::Recording))
    }

    pub fn finish_recording(
        &mut self,
        target: Option<&InsertionTarget>,
    ) -> ChirperResult<Vec<WorkflowEvent>> {
        if self.state != WorkflowState::Recording {
            return Err(ChirperError::Unsupported(format!(
                "cannot finish recording while state is {:?}",
                self.state
            )));
        }

        let mut events = Vec::new();
        let audio = self.audio.stop_recording()?;

        events.extend(self.transition(WorkflowState::Transcribing));
        let transcript = self.asr.transcribe(&audio)?;
        events.push(WorkflowEvent::TranscriptReady(transcript.text.clone()));

        events.extend(self.transition(WorkflowState::Formatting));
        let text = self.formatter.format(&transcript, self.mode)?;

        events.extend(self.transition(WorkflowState::Inserting));
        self.inserter.insert(&text, target)?;
        events.push(WorkflowEvent::TextInserted);

        events.extend(self.transition(WorkflowState::Idle));
        Ok(events)
    }

    fn transition(&mut self, state: WorkflowState) -> Vec<WorkflowEvent> {
        self.state = state;
        vec![WorkflowEvent::StateChanged(state)]
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopFormatter;

impl Formatter for NoopFormatter {
    fn format(&self, transcript: &Transcript, _mode: DictationMode) -> ChirperResult<String> {
        Ok(transcript.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, path::PathBuf, rc::Rc};

    use chirper_core::{CapturedAudio, ChirperResult, InsertionTarget, Transcript};

    use super::*;

    #[derive(Debug, Default)]
    struct FakeAudio {
        started: bool,
    }

    impl AudioSource for FakeAudio {
        fn start_recording(&mut self) -> ChirperResult<()> {
            self.started = true;
            Ok(())
        }

        fn stop_recording(&mut self) -> ChirperResult<CapturedAudio> {
            assert!(self.started);
            self.started = false;
            Ok(CapturedAudio {
                path: PathBuf::from("/tmp/chirper-test.wav"),
                sample_rate_hz: 16_000,
                channels: 1,
            })
        }
    }

    #[derive(Debug)]
    struct FakeAsr;

    impl AsrEngine for FakeAsr {
        fn transcribe(&self, _audio: &CapturedAudio) -> ChirperResult<Transcript> {
            Ok(Transcript {
                text: "hello chirper".to_string(),
                language: Some("en".to_string()),
            })
        }
    }

    #[derive(Debug, Clone)]
    struct FakeInserter {
        inserted: Rc<RefCell<Vec<String>>>,
    }

    impl TextInserter for FakeInserter {
        fn insert(&self, text: &str, _target: Option<&InsertionTarget>) -> ChirperResult<()> {
            self.inserted.borrow_mut().push(text.to_string());
            Ok(())
        }
    }

    #[test]
    fn start_and_finish_recording_runs_full_pipeline() {
        let inserted = Rc::new(RefCell::new(Vec::new()));
        let inserter = FakeInserter {
            inserted: inserted.clone(),
        };
        let mut workflow =
            DictationWorkflow::new(FakeAudio::default(), FakeAsr, NoopFormatter, inserter);

        let start_events = workflow.start_recording().unwrap();
        assert_eq!(
            start_events,
            vec![WorkflowEvent::StateChanged(WorkflowState::Recording)]
        );

        let finish_events = workflow.finish_recording(None).unwrap();

        assert_eq!(workflow.state(), WorkflowState::Idle);
        assert_eq!(inserted.borrow().as_slice(), ["hello chirper"]);
        assert_eq!(
            finish_events,
            vec![
                WorkflowEvent::StateChanged(WorkflowState::Transcribing),
                WorkflowEvent::TranscriptReady("hello chirper".to_string()),
                WorkflowEvent::StateChanged(WorkflowState::Formatting),
                WorkflowEvent::StateChanged(WorkflowState::Inserting),
                WorkflowEvent::TextInserted,
                WorkflowEvent::StateChanged(WorkflowState::Idle),
            ]
        );
    }

    #[test]
    fn finish_before_start_is_rejected() {
        let inserted = Rc::new(RefCell::new(Vec::new()));
        let inserter = FakeInserter { inserted };
        let mut workflow =
            DictationWorkflow::new(FakeAudio::default(), FakeAsr, NoopFormatter, inserter);

        assert!(workflow.finish_recording(None).is_err());
    }
}
