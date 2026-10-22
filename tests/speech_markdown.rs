//! Soft-wrap normalization is for synthesis only; every copied byte keeps its display map.
use herdr_file_viewer::speech::*;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::{
    io,
    sync::{Arc, Mutex},
};

fn lines(text: &str) -> Vec<Line<'static>> {
    text.split('\n').map(|s| Line::raw(s.to_owned())).collect()
}
fn spoken(chunks: &[SpeechChunk]) -> Vec<&str> {
    chunks.iter().map(|c| c.text.as_str()).collect()
}

#[test]
fn soft_line_breaks_are_joined_before_sentence_splitting() {
    let input = lines(
        "A sentence with a visual\nline break in the middle. Another\nsentence finishes here.\n\nA separate paragraph\ncontinues here.",
    );
    let result = markdown_chunks(&input, 0, &[]);
    assert_eq!(
        spoken(&result),
        [
            "A sentence with a visual line break in the middle.",
            "Another sentence finishes here.",
            "A separate paragraph continues here."
        ]
    );
    assert_eq!(result[0].parts.len(), 2);
    assert_eq!(result[1].parts.len(), 2);
    // Do not change code/plain text/diff behavior through the generic entry point.
    assert_eq!(
        spoken(&chunks(&input, 0, &[]))[0],
        "A sentence with a visual"
    );
}

#[test]
fn raw_markdown_keeps_paragraphs_headings_lists_tables_code_and_hard_breaks_separate() {
    let input = lines(
        "# Title\nThe first sentence\ncontinues here.\n\n- A list item\n  continues here.\n- Another item.\n\n| Name | Value |\n| One | Two |\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n\nAn explicit break  \nremains separate.\nA backslash break\\\nalso stays separate.\n\n    indented code one\n    indented code two",
    );
    let result = markdown_source_chunks(&input, 0, &[]);
    assert_eq!(
        spoken(&result),
        [
            "# Title",
            "The first sentence continues here.",
            "- A list item continues here.",
            "- Another item.",
            "| Name | Value |",
            "| One | Two |",
            "```rust",
            "let a = 1;",
            "let b = 2;",
            "An explicit break",
            "remains separate.",
            "A backslash break\\",
            "also stays separate.",
            "indented code one",
            "indented code two"
        ]
    );
}

#[test]
fn renderer_padding_is_not_misinterpreted_as_markdown_hard_breaks() {
    let input = lines("A sentence with a visual        \nline break in the middle.      ");
    assert_eq!(
        spoken(&markdown_chunks(&input, 0, &[])),
        ["A sentence with a visual line break in the middle."]
    );
    assert_eq!(
        spoken(&markdown_source_chunks(&input, 0, &[])),
        ["A sentence with a visual", "line break in the middle."]
    );
}

#[test]
fn rendered_headings_and_tables_are_not_joined_into_prose() {
    let mut input =
        lines("Title\nA paragraph with a soft\nline break.\n\n│ Name │ Value │\n│ One │ Two │");
    input[0] = Line::from(Span::styled(
        "Title",
        Style::new().add_modifier(Modifier::BOLD),
    ));
    assert_eq!(
        spoken(&markdown_chunks(&input, 0, &[])),
        [
            "Title",
            "A paragraph with a soft line break.",
            "│ Name │ Value │",
            "│ One │ Two │"
        ]
    );
}

#[test]
fn starting_mid_fence_remembers_earlier_fence_and_mismatched_fences_do_not_close_it() {
    let input =
        lines("```rust\nlet a = 1;\n~~~\nlet b = 2;\n```\nProse continues\non the next line.");
    assert_eq!(
        spoken(&markdown_source_chunks(&input, 1, &[])),
        [
            "let a = 1;",
            "let b = 2;",
            "Prose continues on the next line."
        ]
    );
}

#[test]
fn joined_unicode_words_map_back_to_their_own_display_line_without_gutter_or_join_spaces() {
    let input = lines("  1 │ Olá world\n  2 │ and café.");
    let result = markdown_chunks(&input, 0, &[6, 6]);
    assert_eq!(spoken(&result), ["Olá world and café."]);
    let chunk = &result[0];
    for part in &chunk.parts {
        let display = input[part.range.line].to_string();
        let len = part.range.end - part.range.start;
        assert_eq!(
            &chunk.text[part.text_start..part.text_start + len],
            &display[part.range.start..part.range.end]
        );
    }
    let at = chunk.text.find("café").unwrap();
    let ranges = chunk.ranges(at, at + "café".len());
    assert_eq!(
        ranges,
        [TextRange {
            line: 1,
            start: 12,
            end: 17
        }]
    );
    let join = chunk.parts[1].text_start - 1;
    assert!(
        chunk.ranges(join, join + 1).is_empty(),
        "synthetic join spaces have no display location"
    );
    assert!(
        chunk.ranges(2, 3).is_empty(),
        "non-UTF-8 boundaries are rejected"
    );
    assert_eq!(
        serde_json::to_value(chunk).unwrap().get("parts"),
        None,
        "source map stays local, protocol unchanged"
    );
}

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

#[test]
fn progress_highlights_words_and_follows_the_second_line_of_a_joined_sentence() {
    let input = lines("Olá world\nand café.");
    let chunks = markdown_chunks(&input, 0, &[]);
    let at = chunks[0].text.find("café").unwrap();
    let fake = Fake::default();
    let mut state = ReadAloud::default();
    state.set_backend(Box::new(fake.clone()), 1.0);
    state.start(chunks);
    let id = fake
        .commands
        .lock()
        .unwrap()
        .iter()
        .find_map(|c| {
            if let SpeechCommand::Speak { id, .. } = c {
                Some(*id)
            } else {
                None
            }
        })
        .unwrap();
    fake.events.lock().unwrap().push(SpeechEvent {
        id,
        event: "progress".into(),
        chunk: 0,
        start: Some(at),
        end: Some(at + 5),
        message: None,
        elapsed: None,
    });
    assert!(state.poll());
    let mark = state.highlight.as_ref().unwrap();
    assert_eq!(
        mark.line, 1,
        "viewport following uses the spoken word's line, not the phrase's first line"
    );
    assert_eq!(mark.word, Some((4, 9)));
    assert_eq!(mark.phrase.len(), 2);
    let drawn = highlight(&input, mark);
    for (original, painted) in input.iter().zip(&drawn) {
        assert_eq!(original.to_string(), painted.to_string());
    }
    assert_eq!(drawn[0].spans[0].style.bg, Some(Color::DarkGray));
    assert!(
        drawn[1]
            .spans
            .iter()
            .any(|s| s.content == "café" && s.style.bg == Some(Color::Yellow))
    );
    assert_eq!(
        input[0].spans[0].style.bg, None,
        "normalization does not modify the original render"
    );
}

#[test]
fn oversized_sentences_prefer_last_comma_before_limit_over_a_later_word_boundary() {
    let clause = format!("{}, {},", "a".repeat(70), "b".repeat(100));
    let text = format!("{clause} {}ends.", "remaining ".repeat(30));
    let result = markdown_chunks(&lines(&text), 0, &[]);
    assert_eq!(result[0].text, clause);
    assert!(result[1].text.starts_with("remaining"));
    assert!(result.iter().all(|c| c.text.chars().count() <= 240));
    assert_eq!(
        result
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        text
    );
}

#[test]
fn commas_do_not_split_sentences_that_already_fit_the_limit() {
    let text = "This sentence has a comma, but it fits. Another, shorter sentence.";
    assert_eq!(
        spoken(&markdown_chunks(&lines(text), 0, &[])),
        [
            "This sentence has a comma, but it fits.",
            "Another, shorter sentence."
        ]
    );
    // A sentence ending exactly at the cap still wins over its earlier comma.
    let exact = format!("A clause, {}.", "é".repeat(229));
    assert_eq!(exact.chars().count(), 240);
    let input = format!("{exact} Next sentence.");
    assert_eq!(
        spoken(&markdown_chunks(&lines(&input), 0, &[])),
        [exact.as_str(), "Next sentence."]
    );
}

#[test]
fn numeric_commas_are_not_clause_boundaries_and_missing_commas_use_word_boundaries() {
    let text = format!("{}3,000 {}ends.", "word ".repeat(38), "word ".repeat(35));
    let result = markdown_chunks(&lines(&text), 0, &[]);
    assert!(result[0].text.contains("3,000"));
    assert!(result[0].text.ends_with("word"));
    assert!(result[0].text.chars().count() > 200);
    assert_eq!(
        result
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        text
    );
}

#[test]
fn unicode_clause_splits_preserve_multiline_byte_maps_and_comma_characters() {
    for comma in ['，', '、'] {
        let input = lines(&format!(
            "{}\n{}{comma}{}",
            "界".repeat(70),
            "界".repeat(30),
            "文".repeat(180)
        ));
        let result = markdown_chunks(&input, 0, &[]);
        assert!(result[0].text.ends_with(comma));
        assert_eq!(result[0].text.chars().count(), 102);
        assert_eq!(result[0].parts.len(), 2);
        assert_eq!(result[1].text, "文".repeat(180));
        for chunk in result {
            for part in chunk.parts {
                let display = input[part.range.line].to_string();
                let len = part.range.end - part.range.start;
                assert_eq!(
                    &chunk.text[part.text_start..part.text_start + len],
                    &display[part.range.start..part.range.end]
                );
            }
        }
    }
}

#[test]
fn comma_exactly_at_limit_stays_in_first_phrase() {
    let clause = format!("{},", "a".repeat(239));
    let text = format!("{clause} More words follow.");
    assert_eq!(
        spoken(&markdown_chunks(&lines(&text), 0, &[])),
        [clause.as_str(), "More words follow."]
    );
}

#[test]
fn long_joined_paragraphs_stay_bounded_and_every_mapping_remains_byte_exact() {
    let input = lines(&"soft wrapped café text\n".repeat(20000));
    let result = markdown_chunks(&input, 0, &[]);
    assert!(result.len() <= 2048);
    assert!(result.iter().map(|c| c.text.len()).sum::<usize>() <= 256 * 1024);
    for chunk in result {
        assert!(chunk.text.chars().count() <= 240);
        assert!(!chunk.parts.is_empty());
        for part in chunk.parts {
            let displayed = input[part.range.line].to_string();
            let len = part.range.end - part.range.start;
            assert_eq!(
                &chunk.text[part.text_start..part.text_start + len],
                &displayed[part.range.start..part.range.end]
            );
        }
    }
}
