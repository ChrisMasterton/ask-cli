//! New tools require a language header and body. Improvements first attempt
//! that format, then retain the existing plain-source fallback on parse failure.

use super::ToolLang;

#[derive(Clone, Copy)]
pub(super) enum DecodeMode<'a> {
    New,
    Improve { name: &'a str, lang: ToolLang },
}

enum HeaderFailure {
    MissingLanguage,
    MissingBody,
}

impl HeaderFailure {
    fn message(self) -> &'static str {
        match self {
            Self::MissingLanguage => {
                "The model's reply did not start with `LANG: bash` or `LANG: python` — nothing saved."
            }
            Self::MissingBody => "The model returned no script body — nothing saved.",
        }
    }
}

enum DecodeState<'a> {
    TrimmingOuterBlanks(Vec<&'a str>),
    RemovingOpeningFence(Vec<&'a str>),
    RemovingClosingFence(Vec<&'a str>),
    ReadingHeader(String),
    CollectingBody {
        source: String,
        lang: ToolLang,
        consumed: usize,
    },
    HeaderRejected {
        source: String,
        reason: HeaderFailure,
    },
    Normalizing {
        lang: ToolLang,
        body: String,
    },
    CheckingLanguage {
        lang: ToolLang,
        body: String,
    },
    ValidatingImprovement {
        lang: ToolLang,
        body: String,
    },
    Complete(ToolLang, String),
    Failed(String),
}

pub(super) fn decode_script(
    content: &str,
    mode: DecodeMode<'_>,
) -> Result<(ToolLang, String), String> {
    let mut state = DecodeState::TrimmingOuterBlanks(content.lines().collect());
    loop {
        state = match state {
            DecodeState::TrimmingOuterBlanks(mut lines) => {
                while lines.first().is_some_and(|line| line.trim().is_empty()) {
                    lines.remove(0);
                }
                while lines.last().is_some_and(|line| line.trim().is_empty()) {
                    lines.pop();
                }
                DecodeState::RemovingOpeningFence(lines)
            }
            DecodeState::RemovingOpeningFence(mut lines) => {
                if lines
                    .first()
                    .is_some_and(|line| line.trim().starts_with("```"))
                {
                    lines.remove(0);
                }
                DecodeState::RemovingClosingFence(lines)
            }
            DecodeState::RemovingClosingFence(mut lines) => {
                // Leading/trailing fence removal is independent and only runs
                // once. Inner fences and their surrounding blanks stay intact.
                if lines
                    .last()
                    .is_some_and(|line| line.trim().starts_with("```"))
                {
                    lines.pop();
                }
                DecodeState::ReadingHeader(lines.join("\n"))
            }
            DecodeState::ReadingHeader(source) => {
                let header = source
                    .lines()
                    .enumerate()
                    .find(|(_, line)| !line.trim().is_empty());
                match header
                    .and_then(|(index, line)| header_language(line).map(|lang| (index + 1, lang)))
                {
                    Some((consumed, lang)) => DecodeState::CollectingBody {
                        source,
                        lang,
                        consumed,
                    },
                    None => DecodeState::HeaderRejected {
                        source,
                        reason: HeaderFailure::MissingLanguage,
                    },
                }
            }
            DecodeState::CollectingBody {
                source,
                lang,
                consumed,
            } => {
                let lines: Vec<_> = source
                    .lines()
                    .skip(consumed)
                    .skip_while(|line| line.trim().is_empty())
                    .collect();
                if lines.is_empty() {
                    DecodeState::HeaderRejected {
                        source,
                        reason: HeaderFailure::MissingBody,
                    }
                } else {
                    DecodeState::Normalizing {
                        lang,
                        body: lines.join("\n"),
                    }
                }
            }
            DecodeState::HeaderRejected { source, reason } => match mode {
                DecodeMode::New => DecodeState::Failed(reason.message().to_string()),
                // Every failed header/body parse keeps the original stripped
                // source, including malformed headers and header-only replies.
                DecodeMode::Improve { lang, .. } => DecodeState::Normalizing { lang, body: source },
            },
            DecodeState::Normalizing { lang, mut body } => {
                if !body.ends_with('\n') {
                    body.push('\n');
                }
                DecodeState::CheckingLanguage { lang, body }
            }
            DecodeState::CheckingLanguage { lang, body } => match mode {
                DecodeMode::New => DecodeState::Complete(lang, body),
                DecodeMode::Improve {
                    name,
                    lang: current,
                } if current != lang => DecodeState::Failed(format!(
                    "The model tried to switch '{name}' from {} to {} — not applied.",
                    current.as_str(),
                    lang.as_str(),
                )),
                DecodeMode::Improve { .. } => DecodeState::ValidatingImprovement { lang, body },
            },
            DecodeState::ValidatingImprovement { lang, body } => {
                if body.trim().is_empty() {
                    DecodeState::Failed(
                        "The model returned no script body — nothing changed.".to_string(),
                    )
                } else {
                    DecodeState::Complete(lang, body)
                }
            }
            DecodeState::Complete(lang, body) => return Ok((lang, body)),
            DecodeState::Failed(error) => return Err(error),
        };
    }
}

fn header_language(line: &str) -> Option<ToolLang> {
    let candidate = line.trim().trim_start_matches(['#', '*']).trim_start();
    let lowercase = candidate.to_lowercase();
    let value = lowercase.strip_prefix("lang:")?;
    ToolLang::from_str(value.trim().trim_end_matches('*').trim_end())
}

#[cfg(test)]
mod tests;
