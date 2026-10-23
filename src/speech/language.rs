//! Per-block spoken language for documents mixing English, Portuguese, Spanish, French
//! and Italian.
//!
//! Short text is unreliable to classify, so a block only switches language on its own
//! when it is long enough or carries an unambiguous letter; otherwise it inherits from
//! its neighbours (a heading from the block it introduces, anything else from the block
//! before it). Code is read in English. Language never changes within a block.

use serde::Serialize;
use whatlang::{Detector, Lang};

use super::chunking::SpeechChunk;

/// Below this many words a block is classified only by unambiguous letters.
const MIN_DETECT_WORDS: usize = 6;
/// Bound the document-wide sample, which only sets the default for undecided blocks.
const DOCUMENT_SAMPLE_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeechLang {
    #[default]
    En,
    Pt,
    Es,
    Fr,
    It,
}

impl SpeechLang {
    pub fn label(self) -> &'static str {
        match self {
            SpeechLang::En => "EN",
            SpeechLang::Pt => "PT",
            SpeechLang::Es => "ES",
            SpeechLang::Fr => "FR",
            SpeechLang::It => "IT",
        }
    }
}

/// What a block is, as far as language inheritance cares. Local-only, never serialized.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockKind {
    #[default]
    Prose,
    Heading,
    Code,
}

fn detector() -> Detector {
    Detector::with_allowlist(vec![Lang::Eng, Lang::Por, Lang::Spa, Lang::Fra, Lang::Ita])
}

fn from_whatlang(lang: Lang) -> SpeechLang {
    match lang {
        Lang::Por => SpeechLang::Pt,
        Lang::Spa => SpeechLang::Es,
        Lang::Fra => SpeechLang::Fr,
        Lang::Ita => SpeechLang::It,
        _ => SpeechLang::En,
    }
}

/// Letters that occur in only one of the supported languages. Shared ones (`ç` is also
/// French, `à`/`è` also Italian, `ù` also French) decide nothing.
fn marker(text: &str) -> Option<SpeechLang> {
    let mut found = None;
    for c in text.chars().flat_map(char::to_lowercase) {
        let lang = match c {
            'ã' | 'õ' => SpeechLang::Pt,
            'ñ' | '¿' | '¡' => SpeechLang::Es,
            'œ' | 'î' | 'ï' | 'û' | 'ë' | 'ÿ' => SpeechLang::Fr,
            'ì' | 'ò' => SpeechLang::It,
            _ => continue,
        };
        match found {
            None => found = Some(lang),
            Some(other) if other != lang => return None,
            _ => {}
        }
    }
    found
}

/// The block's own language, or `None` when it is too short and unmarked to tell.
pub fn classify(text: &str) -> Option<SpeechLang> {
    let words = text
        .split_whitespace()
        .filter(|w| w.chars().any(char::is_alphabetic))
        .count();
    if words >= MIN_DETECT_WORDS {
        return detector().detect_lang(text).map(from_whatlang);
    }
    marker(text)
}

/// Assign `lang` to every chunk. Chunks of one block are contiguous and share `block`.
pub fn assign(chunks: &mut [SpeechChunk]) {
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        match blocks.last_mut() {
            Some((start, end)) if chunks[*start].block == chunk.block => *end = i + 1,
            _ => blocks.push((i, i + 1)),
        }
    }
    let text = |(start, end): (usize, usize)| -> String {
        chunks[start..end]
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let kinds: Vec<BlockKind> = blocks.iter().map(|&(s, _)| chunks[s].kind).collect();
    let own: Vec<Option<SpeechLang>> = blocks
        .iter()
        .zip(&kinds)
        .map(|(&b, kind)| match kind {
            BlockKind::Code => Some(SpeechLang::En),
            _ => classify(&text(b)),
        })
        .collect();

    let mut sample = String::new();
    for (&b, kind) in blocks.iter().zip(&kinds) {
        if *kind != BlockKind::Code && sample.len() < DOCUMENT_SAMPLE_BYTES {
            sample.push_str(&text(b));
            sample.push(' ');
        }
    }
    let document = detector()
        .detect_lang(&sample)
        .map_or(SpeechLang::En, from_whatlang);

    // Code is English but must not drag the surrounding prose's language with it.
    let prose = |i: usize| kinds[i] != BlockKind::Code;
    let mut resolved = vec![document; blocks.len()];
    let mut previous = None;
    for i in 0..blocks.len() {
        resolved[i] = match own[i] {
            Some(lang) => lang,
            None => {
                let next = (i + 1..blocks.len()).find(|&j| prose(j) && own[j].is_some());
                let next = next.and_then(|j| own[j]);
                if kinds[i] == BlockKind::Heading {
                    next.or(previous).unwrap_or(document)
                } else {
                    previous.or(next).unwrap_or(document)
                }
            }
        };
        if prose(i) {
            previous = Some(resolved[i]);
        }
    }
    for (&(start, end), lang) in blocks.iter().zip(resolved) {
        for chunk in &mut chunks[start..end] {
            chunk.lang = lang;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_uses_the_detector_and_short_text_needs_a_marker() {
        let long = |s: &str| classify(s);
        assert_eq!(
            long("To install the plugin, run the command below and restart herdr."),
            Some(SpeechLang::En)
        );
        assert_eq!(
            long("Para instalar o plugin, rode o comando abaixo e reinicie o herdr."),
            Some(SpeechLang::Pt)
        );
        assert_eq!(
            long("Para instalar el plugin, ejecuta el comando de abajo y reinicia herdr."),
            Some(SpeechLang::Es)
        );
        assert_eq!(classify("Instalação"), Some(SpeechLang::Pt));
        assert_eq!(classify("¿Qué tal?"), Some(SpeechLang::Es));
        assert_eq!(
            long("Pour installer le plugin, lancez la commande ci-dessous et redémarrez herdr."),
            Some(SpeechLang::Fr)
        );
        assert_eq!(
            long("Per installare il plugin, esegui il comando qui sotto e riavvia herdr."),
            Some(SpeechLang::It)
        );
        assert_eq!(
            classify("Où est la fenêtre"),
            None,
            "ù and ê are shared, not French-only"
        );
        assert_eq!(classify("Mise à jour coûteuse"), Some(SpeechLang::Fr));
        assert_eq!(classify("Però"), Some(SpeechLang::It));
        assert_eq!(
            classify("Ação e niño"),
            None,
            "conflicting markers decide nothing"
        );
        assert_eq!(classify("Façade"), None, "ç alone is French or Portuguese");
        // Short and unmarked: undecided rather than a coin flip between pt and es.
        assert_eq!(classify("Pressione t para ler"), None);
        assert_eq!(classify("Ok."), None);
    }
}
