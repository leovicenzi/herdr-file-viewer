mod common;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use herdr_file_viewer::{
    controller::{Components, ContentProvider, Controller, RenderResult, RootProviders},
    git::Baseline,
    intent::Intent,
    preview::PreviewSelection,
    speech::*,
    view_policy::ViewMode,
};
use ratatui::text::{Line, Text};
use std::{
    io,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default, Clone)]
struct Fake {
    commands: Arc<Mutex<Vec<SpeechCommand>>>,
    events: Arc<Mutex<Vec<SpeechEvent>>>,
}
impl SpeechBackend for Fake {
    fn send(&mut self, c: SpeechCommand) -> io::Result<()> {
        self.commands.lock().unwrap().push(c);
        Ok(())
    }
    fn poll(&mut self) -> Vec<SpeechEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}
impl Fake {
    fn speak(&self) -> SpeechCommand {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|c| matches!(c, SpeechCommand::Speak { .. }))
            .unwrap()
            .clone()
    }
    fn id(&self) -> u64 {
        match self.speak() {
            SpeechCommand::Speak { id, .. } => id,
            _ => unreachable!(),
        }
    }
    fn progress(&self, chunk: usize, elapsed: f64) {
        self.event(self.id(), "progress", chunk, Some(elapsed));
    }
    fn event(&self, id: u64, kind: &str, chunk: usize, elapsed: Option<f64>) {
        self.events.lock().unwrap().push(SpeechEvent {
            id,
            event: kind.into(),
            chunk,
            start: None,
            end: None,
            message: None,
            elapsed,
        });
    }
}
fn index(text: &str) -> Vec<SpeechChunk> {
    let lines: Vec<_> = text.split('\n').map(Line::raw).collect();
    index_chunks(&lines, 0, &[], TextMode::Markdown)
}
fn state(text: &str) -> (ReadAloud, Fake) {
    let fake = Fake::default();
    let mut s = ReadAloud::default();
    s.set_backend(Box::new(fake.clone()), 1.0);
    s.start(index(text));
    (s, fake)
}
fn speak_fields(fake: &Fake) -> (u64, u64, usize, bool, f64, Vec<SpeechChunk>) {
    match fake.speak() {
        SpeechCommand::Speak {
            id,
            document,
            offset,
            paused,
            speed,
            chunks,
        } => (id, document, offset, paused, speed, chunks),
        _ => unreachable!(),
    }
}

#[test]
fn seeking_before_initial_start_uses_absolute_index_and_rejects_old_progress() {
    let fake = Fake::default();
    let mut s = ReadAloud::default();
    s.set_backend(Box::new(fake.clone()), 1.3);
    s.start_at(index("First sentence. Second sentence. Third sentence."), 2);
    let (old, doc, offset, _, speed, text) = speak_fields(&fake);
    assert_eq!(offset, 2);
    assert_eq!(speed, 1.3);
    assert_eq!(text[0].text, "Third sentence.");
    assert!(s.previous_phrase());
    let (new, same_doc, offset, _, speed, text) = speak_fields(&fake);
    assert_ne!(new, old);
    assert_eq!(doc, same_doc);
    assert_eq!(offset, 1);
    assert_eq!(speed, 1.3);
    assert_eq!(text[0].text, "Second sentence.");
    let target = s.highlight.clone();
    fake.event(old, "progress", 0, Some(10.0));
    fake.event(old, "done", 0, None);
    assert!(!s.poll());
    assert_eq!(s.highlight, target);
    assert_eq!(s.current_chunk(), Some(1));
    fake.progress(0, 0.2);
    assert!(s.poll());
    assert_eq!(s.current_chunk(), Some(1));
    assert!(s.previous_phrase());
    assert_eq!(s.current_chunk(), Some(0));
}

#[test]
fn paused_seeks_are_atomic_keep_speed_and_never_send_an_unpause() {
    let (mut s, fake) = state("One sentence. Two sentences. Three sentences.");
    s.adjust_speed(0.4);
    s.toggle_pause();
    let before = fake.commands.lock().unwrap().len();
    assert!(s.next_phrase());
    let (_, _, offset, paused, speed, _) = speak_fields(&fake);
    assert_eq!(offset, 1);
    assert!(paused);
    assert_eq!(speed, 1.4);
    assert_eq!(s.phase, Phase::Paused);
    assert!(s.highlight.is_some());
    assert!(s.highlight.as_ref().unwrap().word.is_none());
    assert!(
        fake.commands.lock().unwrap()[before..]
            .iter()
            .all(|c| !matches!(c, SpeechCommand::Pause { paused: false, .. }))
    );
    fake.progress(0, 0.0);
    s.poll();
    assert_eq!(s.phase, Phase::Paused);
}

#[test]
fn previous_restarts_after_two_audible_seconds_otherwise_goes_back() {
    let (mut s, fake) = state("First. Second. Third.");
    s.seek(1);
    fake.progress(0, 2.1);
    s.poll();
    s.previous_phrase();
    assert_eq!(s.current_chunk(), Some(1));
    s.previous_phrase();
    assert_eq!(s.current_chunk(), Some(0));
    s.seek(1);
    fake.progress(0, 1.99);
    s.poll();
    s.previous_phrase();
    assert_eq!(s.current_chunk(), Some(0));
    s.seek(1);
    fake.progress(0, f64::NAN);
    s.poll();
    s.previous_phrase();
    assert_eq!(s.current_chunk(), Some(0));
}

#[test]
fn paragraph_skips_follow_soft_wrapped_blocks_not_sentences_or_visual_lines() {
    let (mut s, _) = state(
        "First sentence. Still first\nparagraph.\n\nSecond paragraph. Still second.\n\n# Heading\n\n- A list item\ncontinues here.\n- Next item.",
    );
    s.next_paragraph();
    assert_eq!(s.current_chunk(), Some(2));
    s.next_phrase();
    assert_eq!(s.current_chunk(), Some(3));
    s.previous_paragraph();
    assert_eq!(s.current_chunk(), Some(0));
    s.next_paragraph();
    s.next_paragraph();
    assert_eq!(s.current_chunk(), Some(4));
    s.next_paragraph();
    assert_eq!(s.current_chunk(), Some(5));
    s.next_paragraph();
    assert_eq!(s.current_chunk(), Some(6));
    assert!(!s.next_paragraph());
    assert!(!s.next_phrase());
    s.previous_paragraph();
    assert_eq!(s.current_chunk(), Some(5));
}

#[test]
fn plain_text_keeps_line_phrases_but_blank_lines_define_paragraphs() {
    let lines: Vec<_> = [
        "First line",
        "second line",
        "",
        "Next paragraph",
        "continues",
    ]
    .into_iter()
    .map(Line::raw)
    .collect();
    let chunks = index_chunks(&lines, 0, &[], TextMode::Plain);
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].block, chunks[1].block);
    assert_ne!(chunks[1].block, chunks[2].block);
    assert_eq!(chunks[2].block, chunks[3].block);
    let fake = Fake::default();
    let mut s = ReadAloud::default();
    s.set_backend(Box::new(fake), 1.0);
    s.start(chunks);
    s.next_paragraph();
    assert_eq!(s.current_chunk(), Some(2));
    s.previous_paragraph();
    assert_eq!(s.current_chunk(), Some(0));
}

#[test]
fn a_caret_in_the_second_line_or_sentence_targets_the_containing_phrase() {
    let chunks = index("Opening phrase. Another phrase\ncontinues here. Final sentence.");
    assert_eq!(chunk_at(&chunks, 1, 0), Some(1));
    assert_eq!(chunk_at(&chunks, 1, "continues here. ".len()), Some(2));
    assert_eq!(chunk_at(&chunks, 99, 0), None);
}

#[test]
fn finishing_a_rolling_window_continues_without_losing_document_identity_or_bounds() {
    let input = (0..5000)
        .map(|i| format!("Sentence number {i}. "))
        .collect::<String>();
    let chunks = index(&input);
    assert_eq!(chunks.len(), 5000);
    let fake = Fake::default();
    let mut s = ReadAloud::default();
    s.set_backend(Box::new(fake.clone()), 1.0);
    s.start(chunks);
    let (old, doc, _, _, _, window) = speak_fields(&fake);
    assert_eq!(window.len(), 2048);
    fake.event(old, "done", 0, None);
    assert!(s.poll());
    let (new, new_doc, offset, _, _, window) = speak_fields(&fake);
    assert_ne!(old, new);
    assert_eq!(doc, new_doc);
    assert_eq!(offset, 2048);
    assert_eq!(window.len(), 2048);
    assert!(window.iter().map(|c| c.text.len()).sum::<usize>() <= 256 * 1024);
    fake.progress(1, 0.2);
    s.poll();
    assert_eq!(s.current_chunk(), Some(2049));
    s.seek(4999);
    fake.event(fake.id(), "done", 0, None);
    s.poll();
    assert_eq!(s.phase, Phase::Idle);
    assert_eq!(s.current_chunk(), Some(4999));
    s.previous_phrase();
    assert_eq!(s.current_chunk(), Some(4998));
    s.stop();
    assert_eq!(s.current_chunk(), None);
    assert!(!s.next_phrase());
}

#[test]
fn rolling_requests_enforce_utf8_byte_limit_independently_of_phrase_count() {
    let text = format!("{}。 ", "界".repeat(230)).repeat(600);
    let (s, fake) = state(&text);
    assert!(s.current_chunk().is_some());
    let (_, _, _, _, _, window) = speak_fields(&fake);
    assert!(window.len() < 2048);
    assert!(window.iter().map(|c| c.text.len()).sum::<usize>() <= 256 * 1024);
}

#[test]
fn paused_rolling_window_completion_does_not_unpause_the_next_window() {
    let text = "A sentence. ".repeat(3000);
    let (mut s, fake) = state(&text);
    s.toggle_pause();
    fake.event(fake.id(), "done", 0, None);
    s.poll();
    let (_, _, offset, paused, _, _) = speak_fields(&fake);
    assert_eq!(offset, 2048);
    assert!(paused);
    assert_eq!(s.phase, Phase::Paused);
}

#[test]
fn text_index_is_bounded_separately_from_requests_and_follow_preference_survives_stop() {
    let chunks = index(&"é".repeat(2_200_000));
    assert!(chunks.iter().map(|c| c.text.len()).sum::<usize>() <= 4 * 1024 * 1024);
    assert!(chunks.len() <= 32768);
    let phrases = index(&"Word. ".repeat(40000));
    assert_eq!(phrases.len(), 32768);
    let (mut s, _) = state("A sentence.");
    s.toggle_follow();
    s.stop();
    assert!(!s.follow);
    assert_eq!(s.current_chunk(), None);
}

struct Fixed(String);
impl ContentProvider for Fixed {
    fn render(&self, _: &Path, mode: ViewMode, _: Option<&str>) -> RenderResult {
        RenderResult {
            content: Text::from(self.0.clone()),
            notices: Vec::new(),
            source: (mode == ViewMode::SyntaxContent)
                .then(|| self.0.lines().map(str::to_owned).collect()),
        }
    }
}
fn controller(text: &str) -> (common::TempDir, Controller, Fake) {
    let dir = common::TempDir::new();
    std::fs::write(dir.path().join("alpha.md"), text).unwrap();
    let text = text.to_owned();
    let mut ctrl = Controller::new(
        common::resolved(dir.path().to_path_buf(), false),
        Baseline::Head,
        Components {
            providers: Box::new(move |_| RootProviders {
                git: Arc::new(common::NoopGit),
                content: Box::new(Fixed(text.clone())),
            }),
            editor: Box::new(common::NoopEditor),
            clipboard: Box::new(common::RecordingClipboard::default()),
            renderers: None,
        },
    );
    ctrl.set_content_viewport(80, 4);
    settle(&mut ctrl);
    let fake = Fake::default();
    ctrl.set_speech_backend(Box::new(fake.clone()), 1.0);
    (dir, ctrl, fake)
}
fn settle(ctrl: &mut Controller) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        ctrl.poll();
        if ctrl.active_document().is_some() && !ctrl.view_state().active.rendering {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
const SAMPLE: &str =
    "First sentence.\n\nMiddle phrase\nspans lines. Next sentence.\n\nLast paragraph.";

#[test]
fn follow_off_keeps_browsing_viewport_and_follow_on_reveals_paused_playback_position() {
    let (_dir, mut ctrl, fake) = controller(SAMPLE);
    ctrl.handle(Intent::ToggleSpeech);
    ctrl.handle(Intent::SpeechToggleFollow);
    assert!(!ctrl.speech().follow);
    fake.progress(3, 0.0);
    ctrl.poll();
    assert_eq!(ctrl.speech().highlight.as_ref().unwrap().line, 5);
    assert_eq!(ctrl.content_scroll(), 0);
    ctrl.handle(Intent::ToggleSpeech);
    assert_eq!(ctrl.speech().phase, Phase::Paused);
    ctrl.handle(Intent::SpeechToggleFollow);
    assert!(ctrl.speech().follow);
    assert!(ctrl.content_scroll() > 0);
    assert_eq!(ctrl.speech().phase, Phase::Paused);
}

#[test]
fn selected_read_preserves_pause_speed_and_targets_character_caret_not_viewport() {
    let (_dir, mut ctrl, fake) = controller(SAMPLE);
    ctrl.handle(Intent::ToggleSpeech);
    ctrl.handle(Intent::SpeechFaster);
    ctrl.handle(Intent::ToggleSpeech);
    let (_, doc, _, _, _, _) = speak_fields(&fake);
    let mut selection = PreviewSelection::new(4);
    selection.begin_char(4, "spans lines. ".len(), 6);
    ctrl.active_interaction_mut().selection = Some(selection);
    ctrl.handle(Intent::SpeechReadHere);
    let (_, new_doc, offset, paused, speed, chunks) = speak_fields(&fake);
    assert_eq!(doc, new_doc);
    assert_eq!(offset, 2);
    assert!(paused);
    assert_eq!(speed, 1.1);
    assert_eq!(chunks[0].text, "Next sentence.");
    assert!(ctrl.active_interaction().selection.is_none());
}

#[test]
fn line_select_g_reads_containing_phrase_and_closes_only_that_modal() {
    let (_dir, mut ctrl, fake) = controller(SAMPLE);
    ctrl.handle(Intent::ToggleFocus);
    ctrl.handle(Intent::TreeScrollRight);
    settle(&mut ctrl);
    assert!(ctrl.line_select_active());
    for _ in 0..3 {
        ctrl.handle_line_select_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert_eq!(ctrl.line_selection(), Some((4, 4)));
    ctrl.handle_line_select_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert!(!ctrl.line_select_active());
    let (_, _, offset, paused, _, chunks) = speak_fields(&fake);
    assert_eq!(offset, 1);
    assert!(!paused);
    assert_eq!(chunks[0].text, "Middle phrase spans lines.");
    ctrl.handle(Intent::SpeechPreviousPhrase);
    assert_eq!(ctrl.speech().current_chunk(), Some(0));
}

#[test]
fn visible_current_search_match_is_a_read_here_anchor_but_offscreen_matches_are_not() {
    let (_dir, mut ctrl, fake) = controller(SAMPLE);
    ctrl.active_interaction_mut().search = Some(herdr_file_viewer::infile::SearchState {
        query: "Next".into(),
        current: 0,
        matches: vec![herdr_file_viewer::search::Match {
            line: 3,
            start: 13,
            end: 17,
        }],
    });
    ctrl.handle(Intent::SpeechReadHere);
    assert_eq!(speak_fields(&fake).2, 2);
    ctrl.handle(Intent::StopSpeech);
    ctrl.active_interaction_mut().vertical_scroll = 5;
    ctrl.handle(Intent::SpeechReadHere);
    assert_eq!(speak_fields(&fake).2, 3);
}
