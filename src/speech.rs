//! Optional local read-aloud: displayed-text coordinates, session state, and a JSON-lines
//! subprocess boundary. No file content is passed in argv or written to disk.

use ratatui::text::Line;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};

mod chunking;
#[cfg(test)]
mod mailbox_tests;
pub use chunking::{
    SpeechChunk, SpeechPart, TextMode, TextRange, chunk_at, chunks, index as index_chunks,
    markdown_chunks, markdown_source_chunks,
};

pub const MIN_SPEED: f64 = 0.5;
pub const MAX_SPEED: f64 = 2.5;

pub fn clamp_speed(speed: f64) -> f64 {
    if speed.is_finite() {
        speed.clamp(MIN_SPEED, MAX_SPEED)
    } else {
        1.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechHighlight {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub word: Option<(usize, usize)>,
    /// All displayed runs of a phrase, which may cross Markdown soft line breaks.
    pub phrase: Vec<TextRange>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum SpeechCommand {
    Speak {
        id: u64,
        document: u64,
        offset: usize,
        paused: bool,
        chunks: Vec<SpeechChunk>,
        speed: f64,
    },
    Pause {
        id: u64,
        paused: bool,
    },
    Speed {
        id: u64,
        speed: f64,
    },
    Stop {
        id: u64,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpeechEvent {
    pub id: u64,
    pub event: String,
    #[serde(default)]
    pub chunk: usize,
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub message: Option<String>,
    /// Audible time within the phrase, excluding pauses; optional for custom backends.
    pub elapsed: Option<f64>,
}

/// Injected in tests; live helper startup is lazy and all synthesis/playback is off-thread.
pub trait SpeechBackend {
    fn send(&mut self, command: SpeechCommand) -> io::Result<()>;
    fn poll(&mut self) -> Vec<SpeechEvent>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Loading,
    Speaking,
    Paused,
    Failed,
}

pub struct ReadAloud {
    backend: Option<Box<dyn SpeechBackend>>,
    pub speed: f64,
    pub phase: Phase,
    pub highlight: Option<SpeechHighlight>,
    pub error: Option<String>,
    id: u64,
    document: u64,
    chunks: Vec<SpeechChunk>,
    current: usize,
    window_base: usize,
    window_end: usize,
    elapsed: f64,
    pub follow: bool,
}

impl Default for ReadAloud {
    fn default() -> Self {
        Self {
            backend: None,
            speed: 1.0,
            phase: Phase::Idle,
            highlight: None,
            error: None,
            id: 0,
            document: 0,
            chunks: Vec::new(),
            current: 0,
            window_base: 0,
            window_end: 0,
            elapsed: 0.0,
            follow: true,
        }
    }
}

impl ReadAloud {
    pub fn set_backend(&mut self, backend: Box<dyn SpeechBackend>, speed: f64) {
        self.stop();
        self.backend = Some(backend);
        self.speed = clamp_speed(speed);
    }

    fn send(&mut self, command: SpeechCommand) {
        let result = match &mut self.backend {
            Some(backend) => backend.send(command),
            None => Err(io::Error::other(
                "Read-aloud is not configured; see docs/read-aloud.md",
            )),
        };
        if let Err(err) = result {
            self.phase = Phase::Failed;
            self.highlight = None;
            self.error = Some(err.to_string());
        }
    }

    pub fn start(&mut self, chunks: Vec<SpeechChunk>) {
        self.start_at(chunks, 0);
    }

    pub fn start_at(&mut self, chunks: Vec<SpeechChunk>, at: usize) {
        let paused = self.phase == Phase::Paused;
        self.stop();
        if at >= chunks.len() {
            return;
        }
        self.chunks = chunks;
        self.document = self.id;
        self.play_from(at, paused);
        if !paused {
            self.highlight = None;
        }
    }

    fn play_from(&mut self, at: usize, paused: bool) {
        self.id += 1; // A seek invalidates every queued event from the old playback position.
        self.current = at;
        self.window_base = at;
        self.window_end = at;
        self.elapsed = 0.0;
        let mut bytes = 0;
        for chunk in self.chunks[at..].iter().take(chunking::MAX_CHUNKS) {
            if bytes + chunk.text.len() > chunking::MAX_BYTES {
                break;
            }
            bytes += chunk.text.len();
            self.window_end += 1;
        }
        let chunk = &self.chunks[at];
        let phrase = chunk.ranges(0, chunk.text.len());
        self.highlight = phrase.first().map(|r| SpeechHighlight {
            line: r.line,
            start: r.start,
            end: r.end,
            word: None,
            phrase: phrase.clone(),
        });
        self.error = None;
        self.phase = if paused {
            Phase::Paused
        } else {
            Phase::Loading
        };
        self.send(SpeechCommand::Speak {
            id: self.id,
            document: self.document,
            offset: at,
            paused,
            chunks: self.chunks[at..self.window_end].to_vec(),
            speed: self.speed,
        });
    }

    pub fn current_chunk(&self) -> Option<usize> {
        (!self.chunks.is_empty()).then_some(self.current)
    }

    pub fn seek(&mut self, at: usize) -> bool {
        if at >= self.chunks.len() {
            return false;
        }
        self.play_from(at, self.phase == Phase::Paused);
        true
    }

    pub fn seek_point(&mut self, line: usize, byte: usize) -> bool {
        chunk_at(&self.chunks, line, byte).is_some_and(|at| self.seek(at))
    }

    pub fn next_phrase(&mut self) -> bool {
        self.seek(self.current + 1)
    }

    pub fn previous_phrase(&mut self) -> bool {
        // Without elapsed metadata a custom backend simply goes to the preceding phrase.
        let at = if self.elapsed >= 2.0 {
            self.current
        } else {
            self.current.saturating_sub(1)
        };
        self.seek(at)
    }

    pub fn next_paragraph(&mut self) -> bool {
        let Some(current) = self.chunks.get(self.current) else {
            return false;
        };
        let block = current.block;
        let at = ((self.current + 1)..self.chunks.len()).find(|&i| self.chunks[i].block != block);
        at.is_some_and(|at| self.seek(at))
    }

    pub fn previous_paragraph(&mut self) -> bool {
        let Some(current) = self.chunks.get(self.current) else {
            return false;
        };
        let block = current.block;
        let previous = (0..self.current)
            .rev()
            .find(|&i| self.chunks[i].block != block);
        let at = previous.map_or(0, |i| {
            let block = self.chunks[i].block;
            (0..=i)
                .rev()
                .take_while(|&n| self.chunks[n].block == block)
                .last()
                .unwrap_or(i)
        });
        self.seek(at)
    }

    pub fn toggle_follow(&mut self) {
        self.follow = !self.follow;
    }

    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::Loading | Phase::Speaking | Phase::Paused)
    }

    pub fn toggle_pause(&mut self) {
        if !self.active() {
            return;
        }
        let paused = self.phase != Phase::Paused;
        self.phase = if paused {
            Phase::Paused
        } else {
            Phase::Loading
        };
        self.send(SpeechCommand::Pause {
            id: self.id,
            paused,
        });
    }

    pub fn stop(&mut self) {
        if self.active() || !self.chunks.is_empty() {
            self.send(SpeechCommand::Stop { id: self.id });
        }
        // Invalidates even queued progress/error/done events from an older document.
        self.id += 1;
        self.phase = Phase::Idle;
        self.highlight = None;
        self.error = None;
        self.chunks.clear();
        self.document = 0;
        self.current = 0;
        self.elapsed = 0.0;
        self.window_base = 0;
        self.window_end = 0;
    }

    pub fn adjust_speed(&mut self, delta: f64) {
        self.speed = clamp_speed(((self.speed + delta) * 10.0).round() / 10.0);
        if self.active() {
            self.send(SpeechCommand::Speed {
                id: self.id,
                speed: self.speed,
            });
        }
    }

    pub fn status(&self) -> Option<String> {
        let state = match self.phase {
            Phase::Idle => {
                return self.backend.as_ref().map(|_| {
                    let state = if self.chunks.is_empty() {
                        "Local read-aloud"
                    } else {
                        "Finished"
                    };
                    format!(
                        "{state} · {:.1}× · t read · g here · F follow {}",
                        self.speed,
                        if self.follow { "on" } else { "off" }
                    )
                });
            }
            Phase::Loading => "Preparing local speech",
            Phase::Speaking => "Reading",
            Phase::Paused => "Paused",
            Phase::Failed => {
                return Some(format!(
                    "Read-aloud: {} · t retry · T dismiss",
                    self.error.as_deref().unwrap_or("helper failed")
                ));
            }
        };
        Some(format!(
            "{state} · {:.1}× · {}/{} · F follow {} · t {} · () phrase · K/J block · g here",
            self.speed,
            self.current + 1,
            self.chunks.len(),
            if self.follow { "on" } else { "off" },
            if self.phase == Phase::Paused {
                "resume"
            } else {
                "pause"
            }
        ))
    }

    pub fn poll(&mut self) -> bool {
        let events = self.backend.as_mut().map(|b| b.poll()).unwrap_or_default();
        let mut changed = false;
        for event in events {
            if !self.active() || (event.id != self.id && event.id != 0) {
                continue;
            }
            match event.event.as_str() {
                "progress" if event.id == self.id => {
                    let Some(at) = self
                        .window_base
                        .checked_add(event.chunk)
                        .filter(|&at| at < self.window_end)
                    else {
                        continue;
                    };
                    let Some(chunk) = self.chunks.get(at) else {
                        continue;
                    };
                    self.current = at;
                    self.elapsed = event
                        .elapsed
                        .filter(|s| s.is_finite() && *s >= 0.0)
                        .unwrap_or(0.0);
                    let phrase = chunk.ranges(0, chunk.text.len());
                    let word_ranges = event
                        .start
                        .zip(event.end)
                        .map(|(s, e)| chunk.ranges(s, e))
                        .unwrap_or_default();
                    let active = word_ranges.first().or_else(|| phrase.first());
                    let Some(active) = active else {
                        continue;
                    };
                    // A token spanning synthetic whitespace retains phrase shading rather
                    // than fabricating a contiguous screen range across separate lines.
                    let word = (word_ranges.len() == 1).then_some((active.start, active.end));
                    let on_line = phrase
                        .iter()
                        .find(|r| r.line == active.line)
                        .unwrap_or(active);
                    self.highlight = Some(SpeechHighlight {
                        line: active.line,
                        start: on_line.start,
                        end: on_line.end,
                        word,
                        phrase,
                    });
                    if self.phase != Phase::Paused {
                        self.phase = Phase::Speaking;
                    }
                }
                "done" => {
                    if self.window_end < self.chunks.len() {
                        self.play_from(self.window_end, self.phase == Phase::Paused);
                    } else {
                        self.phase = Phase::Idle;
                        self.highlight = None;
                        // Keep the bounded text index so Previous can revisit finished audio.
                        self.current = self.chunks.len().saturating_sub(1);
                    }
                }
                "error" => {
                    self.phase = Phase::Failed;
                    self.highlight = None;
                    self.error = Some(
                        event
                            .message
                            .unwrap_or_else(|| "speech helper exited".into()),
                    );
                }
                _ => continue,
            }
            changed = true;
        }
        changed
    }
}

/// Non-blocking, coalescing mailbox: one pending text request and at most one
/// pause/speed/stop command. A stalled helper cannot accumulate text on every key repeat.
struct CommandQueue {
    pending: Arc<Mutex<VecDeque<SpeechCommand>>>,
    wake: mpsc::SyncSender<()>,
}

impl CommandQueue {
    fn new() -> (Self, mpsc::Receiver<()>) {
        let (wake, rx) = mpsc::sync_channel(1);
        (
            Self {
                pending: Arc::new(Mutex::new(VecDeque::new())),
                wake,
            },
            rx,
        )
    }

    fn send(&self, command: SpeechCommand) -> io::Result<()> {
        let mut pending = self.pending.lock().unwrap();
        match &command {
            SpeechCommand::Speak { .. } => pending.clear(),
            SpeechCommand::Stop { id } => {
                // If this Speak was never delivered, its Stop ID is unknown to the
                // helper. Deliver both, in order, rather than leave old audio playing.
                pending
                    .retain(|c| matches!(c, SpeechCommand::Speak {id: queued,..} if queued == id));
            }
            SpeechCommand::Pause { .. } => {
                pending.retain(|c| !matches!(c, SpeechCommand::Pause { .. }))
            }
            SpeechCommand::Speed { .. } => {
                pending.retain(|c| !matches!(c, SpeechCommand::Speed { .. }))
            }
        }
        pending.push_back(command);
        drop(pending);
        match self.wake.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(())) => {
                Err(io::Error::other("Local TTS helper disconnected"))
            }
        }
    }
}

/// Owns and reaps one helper. The command writer and event reader never block the UI.
/// No shell, no document path, no document contents in argv, no child cwd inside the viewed repo.
pub struct CommandSpeech {
    argv: Vec<String>,
    child: Option<Child>,
    tx: Option<CommandQueue>,
    rx: Option<mpsc::Receiver<SpeechEvent>>,
}

impl CommandSpeech {
    pub fn new(argv: Vec<String>) -> Self {
        Self {
            argv,
            child: None,
            tx: None,
            rx: None,
        }
    }

    pub fn bundled(plugin_root: &Path) -> Vec<String> {
        vec![
            plugin_root
                .join(".venv-tts/bin/python")
                .to_string_lossy()
                .into_owned(),
            "-u".into(),
            plugin_root
                .join("scripts/read_aloud.py")
                .to_string_lossy()
                .into_owned(),
        ]
    }

    fn spawn(&mut self) -> io::Result<()> {
        if self.argv.is_empty() {
            return Err(io::Error::other("Empty tts_command"));
        }
        let mut command = Command::new(&self.argv[0]);
        command
            .args(&self.argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::proc::in_launch_dir(&mut command);
        // Never import Python modules from the browsed working tree.
        command.env("PYTHONSAFEPATH", "1");
        let mut child = command.spawn().map_err(|e| {
            io::Error::other(format!(
                "Cannot start local TTS: {e}; run scripts/setup-tts.sh"
            ))
        })?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, commands) = CommandQueue::new();
        let pending = tx.pending.clone();
        let (events, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while commands.recv().is_ok() {
                loop {
                    let Some(command) = pending.lock().unwrap().pop_front() else {
                        break;
                    };
                    if serde_json::to_writer(&mut stdin, &command).is_err()
                        || stdin.write_all(b"\n").and_then(|_| stdin.flush()).is_err()
                    {
                        return;
                    }
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = io::BufReader::new(stdout);
            loop {
                // Bounded protocol lines; a broken helper cannot allocate an unbounded string.
                let mut line = Vec::new();
                match reader.by_ref().take(16 * 1024).read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if !line.ends_with(b"\n") => break,
                    Ok(_) => {
                        if let Ok(event) = serde_json::from_slice::<SpeechEvent>(&line)
                            && events.send(event).is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let _ = events.send(SpeechEvent {
                id: 0,
                event: "error".into(),
                chunk: 0,
                start: None,
                end: None,
                message: Some("Local TTS helper exited; check scripts/setup-tts.sh".into()),
                elapsed: None,
            });
        });
        self.child = Some(child);
        self.tx = Some(tx);
        self.rx = Some(rx);
        Ok(())
    }
}

impl SpeechBackend for CommandSpeech {
    fn send(&mut self, command: SpeechCommand) -> io::Result<()> {
        if let Some(child) = &mut self.child
            && child.try_wait()?.is_some()
        {
            self.child = None;
            self.tx = None;
            self.rx = None;
        }
        if self.child.is_none() {
            self.spawn()?;
        }
        self.tx.as_ref().unwrap().send(command)
    }
    fn poll(&mut self) -> Vec<SpeechEvent> {
        self.rx
            .as_ref()
            .map(|rx| rx.try_iter().take(256).collect())
            .unwrap_or_default()
    }
}

impl Drop for CommandSpeech {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Phrase background plus the current model-timed word; composes beneath selection/search.
pub fn highlight(lines: &[Line<'static>], mark: &SpeechHighlight) -> Vec<Line<'static>> {
    let mut result = lines.to_vec();
    let fallback = [TextRange {
        line: mark.line,
        start: mark.start,
        end: mark.end,
    }];
    let ranges = if mark.phrase.is_empty() {
        &fallback[..]
    } else {
        &mark.phrase
    };
    for range in ranges {
        if let Some(line) = result.get_mut(range.line) {
            paint_line(
                line,
                range.start,
                range.end,
                (range.line == mark.line).then_some(mark.word).flatten(),
            );
        }
    }
    result
}

fn paint_line(line: &mut Line<'static>, start: usize, finish: usize, word: Option<(usize, usize)>) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::Span;
    let plain: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let mut offset = 0;
    let mut spans = Vec::new();
    for span in &line.spans {
        let end = offset + span.content.len();
        let mut cuts = vec![offset, end];
        for boundary in [
            Some(start),
            Some(finish),
            word.map(|w| w.0),
            word.map(|w| w.1),
        ]
        .into_iter()
        .flatten()
        {
            if boundary > offset && boundary < end && plain.is_char_boundary(boundary) {
                cuts.push(boundary);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            let mut style = span.style;
            if lo >= start && hi <= finish {
                style = style.patch(Style::new().bg(Color::DarkGray));
            }
            if word.is_some_and(|(a, b)| lo >= a && hi <= b) {
                style = style.patch(
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            }
            spans.push(Span::styled(
                span.content[lo - offset..hi - offset].to_owned(),
                style,
            ));
        }
        offset = end;
    }
    line.spans = spans;
}
