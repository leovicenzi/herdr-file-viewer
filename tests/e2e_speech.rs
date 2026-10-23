//! Real TUI/protocol journey with a deterministic helper; never loads MLX or plays sound.
#![cfg(unix)]
mod common;
use common::{TempDir, viewer_command};
use expectrl::{Eof, Expect, Session};
use std::time::Duration;

#[test]
fn markdown_soft_lines_reach_backend_as_one_sentence() {
    let dir = TempDir::new();
    std::fs::write(
        dir.path().join("alpha.md"),
        "A sentence split\nacross source lines.\n\nAnother paragraph.\n",
    )
    .unwrap();
    let helper = dir.path().join("join.sh");
    std::fs::write(
        &helper,
        r#"while IFS= read -r line; do
  case "$line" in
    *'"cmd":"speak"'*)
      rest=${line#*'"id":'}; id=${rest%%,*}
      case "$line" in
        *'A sentence split across source lines.'*) message=MARKDOWNJOINACK ;;
        *) message=MARKDOWNJOINFAILED ;;
      esac
      printf '{"event":"error","id":%s,"message":"%s"}\n' "$id" "$message"
      ;;
  esac
done
"#,
    )
    .unwrap();
    // Inject a renderer with Glow-like width padding. No installed renderer or audio
    // runtime is needed, and trailing spaces must NOT be treated as hard breaks.
    let renderer = dir.path().join("render.sh");
    std::fs::write(&renderer,
        "cat > /dev/null\nprintf '%-48s\\n' 'A sentence split' 'across source lines.' '' 'Another paragraph.'\n").unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        format!(
            "tts_command = 'sh {}'\nmarkdown = 'sh {}'\n",
            helper.display(),
            renderer.display()
        ),
    )
    .unwrap();
    let mut cmd = viewer_command(dir.path());
    cmd.env_remove("HERDR_FILE_VIEWER_NO_TTS");
    cmd.env("HERDR_PLUGIN_CONFIG_DIR", dir.path());
    cmd.args(["--open", "alpha.md"]);
    let mut session = Session::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(15)));
    session.expect("across").unwrap();
    // Initial Markdown can paint before its width-aware reflow settles. Retry
    // the user action on a bounded condition instead of sleeping for a guessed delay.
    session.set_expect_timeout(Some(Duration::from_millis(200)));
    let mut joined = false;
    for _ in 0..75 {
        session.send("t").unwrap();
        match session.expect("MARKDOWNJOINACK") {
            Ok(_) => {
                joined = true;
                break;
            }
            Err(expectrl::Error::ExpectTimeout) => continue,
            Err(error) => panic!("speech journey failed: {error}"),
        }
    }
    assert!(
        joined,
        "controller joins Markdown display lines before synthesis"
    );
    session.set_expect_timeout(Some(Duration::from_secs(15)));
    // The ACK proves a session exists, so reading mode owns q: the first stops reading,
    // the second (from the observed idle status) quits.
    session.send("q").unwrap();
    session.expect("Local").unwrap();
    session.send("q").unwrap();
    session.expect(Eof).unwrap();
}

#[test]
fn speech_navigation_keys_seek_without_unpausing_and_keep_document_identity() {
    let dir = TempDir::new();
    std::fs::write(
        dir.path().join("alpha.md"),
        "First sentence.\n\nSecond sentence. More second.\n\nThird paragraph.\n",
    )
    .unwrap();
    let log = dir.path().join("commands.jsonl");
    let helper = dir.path().join("navigation.sh");
    std::fs::write(
        &helper,
        r#"while IFS= read -r line; do
  printf '%s\n' "$line" >> "$1"
  case "$line" in
    *'"cmd":"speak"'*)
      rest=${line#*'"id":'}; id=${rest%%,*}
      printf '{"event":"progress","id":%s,"chunk":0,"start":0,"end":5,"elapsed":0.1}\n' "$id"
      ;;
  esac
done
"#,
    )
    .unwrap();
    let renderer = dir.path().join("render.sh");
    std::fs::write(&renderer, "cat\n").unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        format!(
            "tts_command = 'sh {} {}'\nmarkdown = 'sh {}'\n",
            helper.display(),
            log.display(),
            renderer.display()
        ),
    )
    .unwrap();
    let mut cmd = viewer_command(dir.path());
    cmd.env_remove("HERDR_FILE_VIEWER_NO_TTS");
    cmd.env("HERDR_PLUGIN_CONFIG_DIR", dir.path());
    cmd.args(["--open", "alpha.md"]);
    let mut session = Session::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(15)));
    session.expect("Third").unwrap();
    // As in the joining journey, the first preview can precede its settled reflow, and
    // t is refused until it settles. t toggles pause once a session exists, so it is only
    // resent while the helper log proves no `speak` has arrived yet.
    session.set_expect_timeout(Some(Duration::from_secs(1)));
    let spoke = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains(r#""cmd":"speak""#)
    };
    let mut reading = false;
    for _ in 0..15 {
        if !spoke() {
            session.send("t").unwrap();
        }
        match session.expect("Reading") {
            Ok(_) => {
                reading = true;
                break;
            }
            Err(expectrl::Error::ExpectTimeout) => continue,
            Err(error) => panic!("speech start failed: {error}"),
        }
    }
    assert!(reading);
    session.set_expect_timeout(Some(Duration::from_secs(15)));
    session.send("t").unwrap();
    session.expect("Paused").unwrap();
    session.send("=").unwrap();
    let records = || -> Vec<serde_json::Value> {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    };
    let first = records()
        .into_iter()
        .rev()
        .find(|v| v["cmd"] == "speak")
        .unwrap();
    let document = first["document"].as_u64().unwrap();
    let mut previous_id = first["id"].as_u64().unwrap();
    // Reading mode: → next phrase, ↓ next block, ↑ previous block, ← previous phrase,
    // Enter reads from the viewport top.
    for (key, offset) in [
        ("\x1b[C", 1),
        ("\x1b[B", 3),
        ("\x1b[A", 1),
        ("\x1b[D", 0),
        ("\r", 0),
    ] {
        session.send(key).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(seek) = records().into_iter().rev().find(|v| {
                v["cmd"] == "speak" && v["id"].as_u64().is_some_and(|id| id > previous_id)
            }) {
                assert_eq!(seek["offset"], offset);
                assert_eq!(seek["paused"], true);
                assert_eq!(seek["document"], document);
                assert_eq!(seek["speed"], 1.1);
                previous_id = seek["id"].as_u64().unwrap();
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "key {key:?} did not reach helper"
            );
            std::thread::yield_now();
        }
    }
    // q is reading mode's alias for Esc (stop); a lone ESC byte would race the pty's
    // escape-sequence parsing.
    session.send("q").unwrap();
    session.expect("Local").unwrap();
    session.send("q").unwrap();
    session.expect(Eof).unwrap();
}

#[test]
fn speech_keys_route_to_backend_and_status_reports_pause_rate_errors_and_stop() {
    let dir = TempDir::new();
    std::fs::write(
        dir.path().join("alpha.txt"),
        "Hello from the speech test.\n",
    )
    .unwrap();
    let helper = dir.path().join("stub.sh");
    std::fs::write(
        &helper,
        r#"while IFS= read -r line; do
case "$line" in
  *'"cmd":"speak"'*)
    rest=${line#*'"id":'}; id=${rest%%,*}
    printf '{"event":"progress","id":%s,"chunk":0,"start":0,"end":5}\n' "$id" ;;
  *'"cmd":"speed"'*)
    printf '{"event":"error","id":%s,"message":"TEMPOACK"}\n' "$id" ;;
esac
done
"#,
    )
    .unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, format!("tts_command = 'sh {}'\n", helper.display())).unwrap();
    let mut cmd = viewer_command(dir.path());
    cmd.env_remove("HERDR_FILE_VIEWER_NO_TTS");
    cmd.env("HERDR_PLUGIN_CONFIG_DIR", dir.path());
    cmd.args(["--open", "alpha.txt"]);
    let mut session = Session::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(15)));
    session.expect("Hello").unwrap();
    session.send("t").unwrap();
    session
        .expect("Reading")
        .expect("model progress starts the reading status");
    session.send("t").unwrap();
    session
        .expect("Paused")
        .expect("t pauses without losing the file");
    session.send("t").unwrap();
    // Resume transitions via loading; the paused status has to disappear, but no
    // progress need arrive yet. '=' reaches the running backend and the stub ACKs
    // it with a distinctive error, proving the protocol path without a timing guess.
    session.send("=").unwrap();
    session
        .expect("TEMPOACK")
        .expect("rate command reaches helper; errors are visible");
    session.send("q").unwrap();
    session
        .expect("Local")
        .expect("stop clears the error and restores idle status");
    session.send("q").unwrap();
    session
        .expect(Eof)
        .expect("speech helper ownership does not keep viewer alive");
}
