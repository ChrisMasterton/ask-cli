//! Theme values and terminal colors shared by CLI and interactive output.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

pub(crate) struct Theme {
    helper_color: &'static str,
    command_color: &'static str,
    prompt_color: &'static str,
}

const RESET: &str = "\u{001b}[0m";

impl Theme {
    pub(crate) fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self {
                helper_color: "\u{001b}[35m",
                command_color: "\u{001b}[31m",
                prompt_color: "\u{001b}[34m",
            },
            ThemeMode::Dark => Self {
                helper_color: "\u{001b}[36;1m",
                command_color: "\u{001b}[93m",
                prompt_color: "\u{001b}[92m", // bright green - distinct from regular text
            },
        }
    }

    pub(crate) fn helper_text(&self, text: &str) -> String {
        format!("{}{}{}", self.helper_color, text, RESET)
    }

    pub(crate) fn command_text(&self, text: &str) -> String {
        format!("{}{}{}", self.command_color, text, RESET)
    }

    pub(crate) fn prompt_text(&self, text: &str) -> String {
        format!("{}{}{}", self.prompt_color, text, RESET)
    }
}
