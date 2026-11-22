mod common;

use common::{NoopEditor, NoopGit, RecordingClipboard, TempDir, resolved};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use herdr_file_viewer::{
    controller::{Components, ContentProvider, Controller, RenderResult, RootProviders},
    git::Baseline,
    intent::Intent,
    links::{self, Destination, Link},
    open_target::OpenTarget,
    opener::{Opener, OpenerOutcome},
    presenter::PaneGeometry,
    render,
    view_policy::ViewMode,
};
use ratatui::{layout::Rect, text::Text};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const RAW: &str = "α \x1b]8;;https://example.org\x07中文\x1b]8;;\x07 and \x1b]8;;other%20file.md\x07local\x1b]8;;\x07";
struct Markdown;
impl ContentProvider for Markdown {
    fn render(&self, _: &Path, _: ViewMode, _: Option<&str>) -> RenderResult {
        RenderResult {
            content: Text::raw("plain"),
            source: None,
            notices: vec![],
        }
    }
    fn markdown_links(&self, _: &Path, _: Option<u16>) -> Option<(RenderResult, Vec<Link>)> {
        Some((
            RenderResult {
                content: render::to_text(RAW),
                source: None,
                notices: vec![],
            },
            links::from_osc8(RAW),
        ))
    }
}
struct Recorder(Arc<Mutex<Vec<PathBuf>>>);
impl Opener for Recorder {
    fn open(&mut self, path: &Path) -> OpenerOutcome {
        self.0.lock().unwrap().push(path.to_owned());
        OpenerOutcome::Launched
    }
    fn reveal(&mut self, _: &Path) -> OpenerOutcome {
        panic!("must not reveal")
    }
}
fn setup() -> (TempDir, Controller, Arc<Mutex<Vec<PathBuf>>>) {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("README.md"), "source").unwrap();
    std::fs::write(dir.path().join("other file.md"), "other").unwrap();
    let mut ctrl = Controller::new(
        resolved(dir.path().to_owned(), false),
        Baseline::Head,
        Components {
            providers: Box::new(|_| RootProviders {
                git: Arc::new(NoopGit),
                content: Box::new(Markdown),
            }),
            editor: Box::new(NoopEditor),
            clipboard: Box::new(RecordingClipboard::default()),
            renderers: None,
        },
    );
    ctrl.apply_open_target(&OpenTarget {
        path: "README.md".into(),
        line: None,
        end_line: None,
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while ctrl.active_document().is_none() {
        ctrl.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    ctrl.set_content_viewport(60, 10);
    ctrl.set_pane_geometry(PaneGeometry {
        content_inner: Some(Rect::new(10, 1, 60, 10)),
        ..Default::default()
    });
    let opened = Arc::new(Mutex::new(vec![]));
    ctrl.set_opener(Box::new(Recorder(opened.clone())));
    (dir, ctrl, opened)
}
fn mouse(ctrl: &mut Controller, kind: MouseEventKind, col: u16, modifiers: KeyModifiers) {
    ctrl.handle_mouse(MouseEvent {
        kind,
        column: col,
        row: 1,
        modifiers,
    });
}
#[test]
fn click_unicode_link_but_drag_and_shift_do_not_launch() {
    let (_dir, mut ctrl, opened) = setup();
    // α and space take two cells; the link's first CJK character starts at x=12.
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        mouse(&mut ctrl, kind, 12, KeyModifiers::NONE);
    }
    assert_eq!(
        *opened.lock().unwrap(),
        vec![PathBuf::from("https://example.org")]
    );
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        let col = if matches!(kind, MouseEventKind::Down(_)) {
            12
        } else {
            16
        };
        mouse(&mut ctrl, kind, col, KeyModifiers::NONE);
    }
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        mouse(&mut ctrl, kind, 12, KeyModifiers::SHIFT);
    }
    // Even a drag that returns to the original caret is not a click.
    for (kind, col) in [
        (MouseEventKind::Down(MouseButton::Left), 12),
        (MouseEventKind::Drag(MouseButton::Left), 16),
        (MouseEventKind::Drag(MouseButton::Left), 12),
        (MouseEventKind::Up(MouseButton::Left), 12),
    ] {
        mouse(&mut ctrl, kind, col, KeyModifiers::NONE);
    }
    assert_eq!(opened.lock().unwrap().len(), 1);
}
#[test]
fn keyboard_cycles_links_and_opens_local_file_without_os_handoff() {
    let (dir, mut ctrl, opened) = setup();
    ctrl.handle(Intent::NextLink);
    ctrl.handle(Intent::Activate);
    assert_eq!(opened.lock().unwrap().len(), 1);
    ctrl.handle(Intent::NextLink);
    ctrl.handle(Intent::Activate);
    assert_eq!(opened.lock().unwrap().len(), 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while ctrl.active_document().is_none() {
        ctrl.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        ctrl.active_document().unwrap().origin().absolute_path(),
        dir.path().join("other file.md")
    );
}
#[test]
fn switching_view_clears_old_link_metadata_synchronously() {
    let (_dir, mut ctrl, opened) = setup();
    ctrl.handle(Intent::NextLink);
    ctrl.handle(Intent::CycleView);
    ctrl.handle(Intent::NextLink);
    ctrl.handle(Intent::Activate);
    assert!(opened.lock().unwrap().is_empty());
}

#[test]
fn local_links_decode_relative_to_document_and_reject_escape() {
    let dir = TempDir::new();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("a b.md"), "x").unwrap();
    let source = dir.path().join("docs/start.md");
    assert_eq!(
        links::destination(dir.path(), &source, "../a%20b.md#part"),
        Ok(Destination::Local(dir.path().join("a b.md")))
    );
    assert!(links::destination(dir.path(), &source, "%00bad").is_err());
    let outside = TempDir::new();
    std::fs::write(outside.path().join("secret.md"), "x").unwrap();
    assert!(
        links::destination(
            dir.path(),
            &source,
            outside.path().join("secret.md").to_str().unwrap()
        )
        .is_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        assert!(links::destination(dir.path(), &source, "../escape/secret.md").is_err());
    }
}
