mod common;

use herdr_file_viewer::speech::*;
use herdr_file_viewer::{
    config,
    controller::{Components, Controller, RootProviders},
    git::Baseline,
    intent::Intent,
};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::{
    io,
    sync::{Arc, Mutex},
};

#[derive(Default, Clone)]
struct Fake {
    commands: Arc<Mutex<Vec<SpeechCommand>>>,
    events: Arc<Mutex<Vec<SpeechEvent>>>,
}
impl SpeechBackend for Fake {
    fn send(&mut self, command: SpeechCommand) -> io::Result<()> {
        self.commands.lock().unwrap().push(command);
        Ok(())
    }
    fn poll(&mut self) -> Vec<SpeechEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}
fn phrase() -> SpeechChunk {
    SpeechChunk {
        text: "Olá world.".into(),
        line: 2,
        start: 4,
        end: 15,
        parts: Vec::new(),
        block: 2,
        kind: BlockKind::Prose,
        lang: SpeechLang::Pt,
    }
}
fn session(fake: &Fake) -> u64 {
    fake.commands
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|c| match c {
            SpeechCommand::Speak { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap()
}
fn event(id: u64, kind: &str, start: Option<usize>, end: Option<usize>) -> SpeechEvent {
    SpeechEvent {
        id,
        event: kind.into(),
        chunk: 0,
        start,
        end,
        message: None,
        elapsed: None,
    }
}

#[test]
fn chunking_preserves_utf8_coordinates_and_skips_gutters_and_decoration() {
    let lines = vec![
        Line::raw("  1 │ Olá world. Next sentence!"),
        Line::raw("──────"),
        Line::raw("日本語です。 More."),
    ];
    let result = chunks(&lines, 0, &[6, 0, 0]);
    assert_eq!(
        result.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        ["Olá world.", "Next sentence!", "日本語です。", "More."]
    );
    for chunk in result {
        let plain: String = lines[chunk.line]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(&plain[chunk.start..chunk.end], chunk.text);
    }
    assert!(chunks(&lines, 99, &[]).is_empty());
}

#[test]
fn long_lines_split_without_losing_unicode_or_exceeding_bounds() {
    let input = "é".repeat(3000);
    let lines = vec![Line::raw(input.clone())];
    let result = chunks(&lines, 0, &[]);
    assert_eq!(
        result.iter().map(|c| c.text.as_str()).collect::<String>(),
        input
    );
    assert!(result.iter().all(|c| c.text.chars().count() <= 240));
    assert!(chunks(&[Line::raw("hello. ".repeat(10000))], 0, &[]).len() <= 2048);
    assert!(
        chunks(&[Line::raw("a".repeat(500000))], 0, &[])
            .iter()
            .map(|c| c.text.len())
            .sum::<usize>()
            <= 256 * 1024
    );
}

#[test]
fn pause_resume_rate_and_stop_are_explicit_commands() {
    let fake = Fake::default();
    let mut state = ReadAloud::default();
    state.set_backend(Box::new(fake.clone()), 1.0);
    state.start(vec![phrase()]);
    assert_eq!(state.phase, Phase::Loading);
    state.toggle_pause();
    assert_eq!(state.phase, Phase::Paused);
    state.toggle_pause();
    assert_eq!(state.phase, Phase::Loading);
    state.adjust_speed(0.1);
    assert_eq!(state.speed, 1.1);
    state.adjust_speed(99.0);
    assert_eq!(state.speed, MAX_SPEED);
    state.adjust_speed(-99.0);
    assert_eq!(state.speed, MIN_SPEED);
    state.stop();
    assert_eq!(state.phase, Phase::Idle);
    let commands = fake.commands.lock().unwrap();
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, SpeechCommand::Pause { paused: true, .. }))
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, SpeechCommand::Pause { paused: false, .. }))
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, SpeechCommand::Speed { speed, .. } if *speed == 1.1))
    );
    assert!(matches!(commands.last(), Some(SpeechCommand::Stop { .. })));
}

#[test]
fn old_session_progress_and_done_cannot_repaint_or_stop_new_session() {
    let fake = Fake::default();
    let mut state = ReadAloud::default();
    state.set_backend(Box::new(fake.clone()), 1.0);
    state.start(vec![phrase()]);
    let old = session(&fake);
    state.start(vec![phrase()]);
    let current = session(&fake);
    fake.events.lock().unwrap().extend([
        event(old, "progress", Some(0), Some(4)),
        event(old, "done", None, None),
    ]);
    assert!(!state.poll());
    assert_eq!(state.phase, Phase::Loading);
    assert!(state.highlight.is_none());
    fake.events
        .lock()
        .unwrap()
        .push(event(current, "progress", Some(0), Some(4)));
    assert!(state.poll());
    assert_eq!(state.highlight.as_ref().unwrap().word, Some((4, 8)));
    state.stop();
    fake.events
        .lock()
        .unwrap()
        .push(event(current, "progress", Some(0), Some(4)));
    assert!(!state.poll());
    assert!(state.highlight.is_none());
}

#[test]
fn invalid_word_ranges_degrade_to_phrase_and_invalid_chunks_are_ignored() {
    let fake = Fake::default();
    let mut state = ReadAloud::default();
    state.set_backend(Box::new(fake.clone()), 1.0);
    state.start(vec![phrase()]);
    let id = session(&fake);
    for (start, end) in [(0, 3), (9, 999), (5, 2), (4, 4)] {
        fake.events
            .lock()
            .unwrap()
            .push(event(id, "progress", Some(start), Some(end)));
        state.poll();
        assert_eq!(state.highlight.as_ref().unwrap().word, None);
    }
    let before = state.highlight.clone();
    let mut invalid = event(id, "progress", Some(0), Some(4));
    invalid.chunk = 999;
    fake.events.lock().unwrap().push(invalid);
    assert!(!state.poll());
    assert_eq!(state.highlight, before);
}

#[test]
fn errors_are_visible_and_pause_preserves_progress() {
    let fake = Fake::default();
    let mut state = ReadAloud::default();
    state.set_backend(Box::new(fake.clone()), 1.0);
    state.start(vec![phrase()]);
    let id = session(&fake);
    state.toggle_pause();
    fake.events
        .lock()
        .unwrap()
        .push(event(id, "progress", Some(0), Some(4)));
    state.poll();
    assert_eq!(state.phase, Phase::Paused);
    let mut error = event(id, "error", None, None);
    error.message = Some("No audio device".into());
    fake.events.lock().unwrap().push(error);
    state.poll();
    assert!(state.status().unwrap().contains("No audio device"));
    assert!(state.highlight.is_none());
}

#[test]
fn highlighting_crosses_styled_spans_without_changing_text_or_unicode() {
    let lines = vec![Line::from(vec![
        Span::styled("Olá ", Style::new().fg(Color::Green)),
        Span::raw("world."),
    ])];
    let mark = SpeechHighlight {
        line: 0,
        start: 0,
        end: 11,
        word: Some((5, 10)),
        phrase: Vec::new(),
    };
    let highlighted = highlight(&lines, &mark);
    assert_eq!(highlighted[0].to_string(), "Olá world.");
    assert_eq!(lines[0].spans[0].style.bg, None);
    assert_eq!(highlighted[0].spans[0].style.bg, Some(Color::DarkGray));
    assert!(
        highlighted[0]
            .spans
            .iter()
            .any(|s| s.content == "world" && s.style.bg == Some(Color::Yellow))
    );
    let invalid = SpeechHighlight {
        word: Some((3, 999)),
        ..mark
    };
    assert_eq!(highlight(&lines, &invalid)[0].to_string(), "Olá world.");
}

#[test]
fn config_speed_is_finite_clamped_and_commands_are_argv_not_shells() {
    for (raw, expected) in [
        ("0.1", 0.5),
        ("9.0", 2.5),
        ("nan", 1.0),
        ("inf", 1.0),
        ("1.4", 1.4),
    ] {
        let (cfg, _) = config::parse_config(&format!(
            "tts_speed = {raw}\ntts_command = '\"/path with spaces/python\" -u /helper.py'"
        ));
        let eff = config::resolve(&cfg, |_| None);
        assert_eq!(eff.tts_speed, expected);
        assert_eq!(
            eff.tts_command.unwrap(),
            ["/path with spaces/python", "-u", "/helper.py"]
        );
    }
}

#[test]
fn controller_stops_audio_synchronously_when_document_changes_and_ignores_stale_events() {
    let dir = common::TempDir::new();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();
    let mut ctrl = Controller::new(
        common::resolved(dir.path().to_path_buf(), false),
        Baseline::Head,
        Components {
            providers: Box::new(|_| RootProviders {
                git: Arc::new(common::NoopGit),
                content: Box::new(common::NoopContent),
            }),
            editor: Box::new(common::NoopEditor),
            clipboard: Box::new(common::RecordingClipboard::default()),
            renderers: None,
        },
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while ctrl.active_document().is_none() {
        ctrl.poll();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let fake = Fake::default();
    ctrl.set_speech_backend(Box::new(fake.clone()), 1.0);
    ctrl.handle(Intent::ToggleSpeech);
    assert!(ctrl.speech().active());
    let id = session(&fake);
    ctrl.handle(Intent::ToggleFocus); // reading puts focus in content; Tab returns to tree
    ctrl.handle(Intent::NavDown);
    assert_eq!(ctrl.speech().phase, Phase::Idle);
    assert!(matches!(
        fake.commands.lock().unwrap().last(),
        Some(SpeechCommand::Stop { .. })
    ));
    fake.events
        .lock()
        .unwrap()
        .push(event(id, "progress", Some(0), Some(4)));
    ctrl.poll();
    assert!(ctrl.view_state().active.speech.is_none());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "a"
    );
}
