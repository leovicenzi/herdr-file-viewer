//! User-activated Markdown hyperlinks. No destination is opened during rendering.
use super::*;

impl Controller {
    pub(super) fn follow_link_at(&mut self, line: usize, col: usize) -> Effects {
        let target = self
            .active_links
            .iter()
            .find(|l| l.line + 1 == line && col >= l.start && col < l.end)
            .map(|l| l.target.clone());
        let Some(target) = target else {
            return Effects::redraw();
        };
        let Some(document) = self.active_document() else {
            return Effects::noop();
        };
        let source = document.origin().absolute_path().to_owned();
        match crate::links::destination(&self.root, &source, &target) {
            Ok(crate::links::Destination::Local(path)) => {
                self.exit_line_select();
                self.apply_open_target(&crate::open_target::OpenTarget {
                    path: path.to_string_lossy().into_owned(),
                    line: None,
                    end_line: None,
                });
            }
            Ok(crate::links::Destination::Web(uri)) => {
                if let Some(opener) = self.opener.as_mut() {
                    match opener.open(Path::new(&uri)) {
                        crate::opener::OpenerOutcome::Launched => {}
                        crate::opener::OpenerOutcome::NotLaunched(reason)
                        | crate::opener::OpenerOutcome::NonZeroExit(reason) => {
                            self.set_flash(format!("Could not open link: {reason}"))
                        }
                    }
                }
            }
            Err(reason) => self.set_flash(reason),
        }
        Effects::redraw()
    }

    /// Cycle every link, including multiple destinations on the same line.
    pub(super) fn next_markdown_link(&mut self) -> Effects {
        if self.focus == Focus::Pinned {
            return Effects::noop();
        }
        let position = self
            .active_interaction
            .selection
            .as_ref()
            .map(|s| s.char_span().0);
        let current = self
            .active_links
            .iter()
            .position(|l| Some((l.line + 1, l.start)) == position);
        let next = match current {
            Some(i) => self.active_links.get((i + 1) % self.active_links.len()),
            None => self
                .active_links
                .iter()
                .find(|l| {
                    self.content_row_of_line(l.line + 1)
                        >= self.active_interaction.vertical_scroll as usize
                })
                .or_else(|| self.active_links.first()),
        }
        .cloned();
        let Some(link) = next else {
            self.set_flash("No clickable links in this rendered view");
            return Effects::redraw();
        };
        let last = self.active_lines().len();
        let mut selection = PreviewSelection::new(link.line + 1);
        selection.begin_char(link.line + 1, link.start, last);
        selection.drag_char(link.line + 1, link.end, last);
        self.active_interaction.selection = Some(selection);
        self.focus = Focus::Content;
        self.set_flash("Enter: open selected link · o: next link · Esc: cancel");
        self.active_interaction.vertical_scroll = self.content_row_of_line(link.line + 1) as u16;
        self.clamp_active_interaction();
        Effects::redraw()
    }

    pub(super) fn selected_link_position(&self) -> Option<(usize, usize)> {
        let selection = self.active_interaction.selection.as_ref()?;
        let (a, b) = selection.char_span();
        self.active_links
            .iter()
            .find(|l| a == (l.line + 1, l.start) && b == (l.line + 1, l.end))
            .map(|_| a)
    }
}
