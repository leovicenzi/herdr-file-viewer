//! Read-aloud orchestration. Coordinates always refer to the settled active render.
use super::*;
use crate::intent::ReadingAction;
use crate::speech::{ReadAloud, SpeechBackend, TextMode};

impl Controller {
    pub fn set_speech_backend(&mut self, backend: Box<dyn SpeechBackend>, speed: f64) {
        self.speech.set_backend(backend, speed);
    }

    pub fn speech(&self) -> &ReadAloud {
        &self.speech
    }

    /// Reading mode lasts from the first `t` until Esc stops it, including a finished or failed
    /// session (so ← can revisit the end and Esc dismisses the error).
    pub fn reading_mode_active(&self) -> bool {
        self.speech.in_session()
    }

    pub fn handle_reading(&mut self, action: ReadingAction) -> Effects {
        match action {
            ReadingAction::TogglePause => self.toggle_speech(),
            ReadingAction::Stop => {
                self.speech.stop();
                Effects::redraw()
            }
            ReadingAction::Faster => {
                self.speech.adjust_speed(0.1);
                Effects::redraw()
            }
            ReadingAction::Slower => {
                self.speech.adjust_speed(-0.1);
                Effects::redraw()
            }
            ReadingAction::ReadHere => self.read_speech_here(),
            ReadingAction::ToggleFollow => self.toggle_speech_follow(),
            ReadingAction::NextPhrase
            | ReadingAction::PreviousPhrase
            | ReadingAction::NextParagraph
            | ReadingAction::PreviousParagraph => self.skip_speech(action),
        }
    }

    fn speech_top_line(&self) -> usize {
        self.line_at_content_row(self.active_interaction.vertical_scroll as usize)
            .saturating_sub(1)
    }

    pub(super) fn toggle_speech(&mut self) -> Effects {
        if self.speech.active() {
            self.speech.toggle_pause();
            return Effects::redraw();
        }
        if self.read_speech_at(self.speech_top_line(), 0) {
            self.focus = Focus::Content;
            self.follow_speech();
        }
        Effects::redraw()
    }

    /// Read the containing phrase, not just the selected fragment. A selection is a
    /// starting anchor, never a new document/audio clip; subsequent text continues normally.
    pub(super) fn read_speech_here(&mut self) -> Effects {
        self.action_notice = None;
        let point = self
            .modal
            .line_select()
            .or(self.active_interaction.selection.as_ref())
            .map(|s| {
                let (line, col) = if s.is_char_mode() {
                    s.char_span().0
                } else {
                    (s.selection().0, 0)
                };
                let line = line.saturating_sub(1);
                let plain = self
                    .active_lines()
                    .get(line)
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let byte = plain
                    .char_indices()
                    .nth(col)
                    .map_or(plain.len(), |(i, _)| i);
                (line, byte)
            })
            .or_else(|| {
                let search = self.active_interaction.search.as_ref()?;
                let m = search.matches.get(search.current)?;
                let top = self.speech_top_line();
                let bottom = self
                    .line_at_content_row(
                        self.active_interaction.vertical_scroll as usize
                            + self.active_interaction.viewport_height.saturating_sub(1) as usize,
                    )
                    .saturating_sub(1);
                (m.line >= top && m.line <= bottom).then_some((m.line, m.start))
            })
            .unwrap_or_else(|| (self.speech_top_line(), 0));
        if self.read_speech_at(point.0, point.1) {
            if self.line_select_active() {
                self.exit_line_select();
            }
            self.active_interaction.selection = None;
            self.drag = None;
            self.focus = Focus::Content;
            self.follow_speech();
        }
        Effects::redraw()
    }

    fn read_speech_at(&mut self, line: usize, byte: usize) -> bool {
        if self.focus == Focus::Pinned {
            self.action_notice = Some("Read-aloud targets the active preview, not the pin".into());
            return false;
        }
        if self.active_document().is_none() || self.latest_seq != self.applied_seq {
            self.action_notice =
                Some("Select a text file and wait for its preview before reading".into());
            return false;
        }
        // Reuse the settled document index and cache identity for in-document seeks.
        if self.speech.seek_point(line, byte) {
            return true;
        }
        let document = self
            .active_document()
            .expect("settled document checked above");
        let mode = match document.presentation().view_mode() {
            ViewMode::RenderedMarkdown if is_markdown(document.origin().absolute_path()) => {
                TextMode::Markdown
            }
            ViewMode::SyntaxContent if is_markdown(document.origin().absolute_path()) => {
                TextMode::MarkdownSource
            }
            _ => TextMode::Plain,
        };
        let gutters: Vec<_> = (1..=self.active_lines().len())
            .map(|n| self.content_gutter_len(n))
            .collect();
        let mut chunks = crate::speech::index_chunks(self.active_lines(), 0, &gutters, mode);
        let mut at = crate::speech::chunk_at(&chunks, line, byte);
        if at.is_none() && line > 0 {
            // Explicit starts beyond the bounded prefix get a new local text index.
            chunks = crate::speech::index_chunks(self.active_lines(), line, &gutters, mode);
            at = crate::speech::chunk_at(&chunks, line, byte);
        }
        if let Some(at) = at {
            self.speech.start_at(chunks, at);
            true
        } else {
            self.action_notice = Some("No readable phrase at or after this passage".into());
            false
        }
    }

    fn skip_speech(&mut self, action: ReadingAction) -> Effects {
        let moved = match action {
            ReadingAction::NextPhrase => self.speech.next_phrase(),
            ReadingAction::PreviousPhrase => self.speech.previous_phrase(),
            ReadingAction::NextParagraph => self.speech.next_paragraph(),
            ReadingAction::PreviousParagraph => self.speech.previous_paragraph(),
            _ => false,
        };
        if moved {
            self.follow_speech();
        } else {
            self.action_notice = Some(
                if self.speech.current_chunk().is_none() {
                    "Start read-aloud with t first"
                } else {
                    "End of indexed preview; scroll and press Enter to read elsewhere"
                }
                .into(),
            );
        }
        Effects::redraw()
    }

    pub(super) fn toggle_speech_follow(&mut self) -> Effects {
        self.speech.toggle_follow();
        self.follow_speech();
        Effects::redraw()
    }

    fn follow_speech(&mut self) {
        // Browsing has its own cursor. Modals/dragging must not fight the playback cursor.
        if !self.speech.follow || !matches!(self.modal, Modal::None) || self.drag.is_some() {
            return;
        }
        if let Some(mark) = &self.speech.highlight {
            let line = mark.line;
            let row = self.content_row_of_line(line + 1);
            let height = self.active_interaction.viewport_height.max(1) as usize;
            let scroll = self.active_interaction.vertical_scroll as usize;
            if row < scroll || row >= scroll + height {
                self.scroll_to_line(line + 1);
            }
        }
    }

    pub(super) fn poll_speech(&mut self) -> bool {
        if !self.speech.poll() {
            return false;
        }
        self.follow_speech();
        true
    }
}
