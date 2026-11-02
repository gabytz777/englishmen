use std::fmt;

/// A position in an Englishmen source file.
///
/// `col` is 1-based and counted in characters. `end_col` is optional: when it
/// is present, diagnostics underline the whole token or expression instead of
/// a single caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: usize,
    pub col: usize,
    pub end_col: Option<usize>,
}

impl Span {
    pub fn new(line: usize, col: usize) -> Self {
        Span {
            line,
            col,
            end_col: None,
        }
    }

    /// Mark the span as covering `len` characters starting at `col`.
    pub fn with_len(mut self, len: usize) -> Self {
        if len > 1 {
            self.end_col = Some(self.col + len - 1);
        }
        self
    }

    /// Mark the span as ending at an inclusive column.
    pub fn with_end(mut self, end_col: usize) -> Self {
        if end_col > self.col {
            self.end_col = Some(end_col);
        }
        self
    }
}

/// Broad categories of errors. The category is stable even when the wording
/// of the message changes, and it lets future features (such as `try`/`catch`)
/// decide what can be handled by a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorKind {
    Syntax,
    Name,
    Type,
    Arity,
    Index,
    Io,
    Import,
    #[default]
    Runtime,
    User,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Syntax => "syntax",
            ErrorKind::Name => "name",
            ErrorKind::Type => "type",
            ErrorKind::Arity => "arity",
            ErrorKind::Index => "index",
            ErrorKind::Io => "io",
            ErrorKind::Import => "import",
            ErrorKind::Runtime => "runtime",
            ErrorKind::User => "user",
        }
    }
}

/// A rich, user-facing error produced anywhere in the pipeline.
#[derive(Debug, Clone)]
pub struct EmError {
    pub message: String,
    pub span: Option<Span>,
    pub hint: Option<String>,
    pub kind: ErrorKind,
    /// The file this error belongs to. Set when a diagnostic comes from an
    /// imported module rather than the file currently being rendered.
    pub file: Option<String>,
    /// Extra explanatory lines shown below the source excerpt.
    pub notes: Vec<String>,
    /// An optional second location, used for notes such as "first defined here".
    pub secondary: Option<(Span, String)>,
}

impl EmError {
    pub fn new(message: impl Into<String>) -> Self {
        EmError {
            message: message.into(),
            span: None,
            hint: None,
            kind: ErrorKind::Runtime,
            file: None,
            notes: Vec::new(),
            secondary: None,
        }
    }

    pub fn syntax(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::Syntax)
    }

    pub fn name(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::Name)
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::Type)
    }

    pub fn arity(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::Arity)
    }

    pub fn index(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::Index)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::Io)
    }

    pub fn import(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::Import)
    }

    pub fn user(message: impl Into<String>) -> Self {
        Self::new(message).with_kind(ErrorKind::User)
    }

    pub fn with_kind(mut self, kind: ErrorKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_secondary(mut self, span: Span, label: impl Into<String>) -> Self {
        self.secondary = Some((span, label.into()));
        self
    }

    /// Render a nicely formatted, source-aware error message.
    pub fn render(&self, filename: &str, source: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("Error: {}\n", self.message));
        let file = self.file.as_deref().unwrap_or(filename);
        if let Some(span) = self.span {
            out.push_str(&format!("  --> {}:{}:{}\n", file, span.line, span.col));
            Self::render_excerpt(&mut out, source, span, self.span_label());
            if let Some((secondary_span, label)) = &self.secondary {
                Self::render_excerpt(&mut out, source, *secondary_span, Some(label.as_str()));
            }
        }
        if !self.notes.is_empty() {
            out.push('\n');
            for note in &self.notes {
                out.push_str("  = note: ");
                out.push_str(note);
                out.push('\n');
            }
        }
        if let Some(hint) = &self.hint {
            out.push('\n');
            out.push_str(hint);
            out.push('\n');
        }
        out
    }

    fn span_label(&self) -> Option<&str> {
        None
    }

    fn render_excerpt(out: &mut String, source: &str, span: Span, label: Option<&str>) {
        // Compiled bytecode carries positions but not the source it came
        // from, so the excerpt is skipped rather than left blank.
        let Some(line_text) = source.lines().nth(span.line.saturating_sub(1)) else {
            return;
        };
        out.push('\n');
        let line_num = span.line.to_string();
        let gutter = " ".repeat(line_num.len());
        let expanded = expand_tabs(line_text);
        out.push_str(&format!("{} | {}\n", line_num, expanded));
        let pad = display_width_until(line_text, span.col);
        let caret_len = span
            .end_col
            .map(|end| end.saturating_sub(span.col) + 1)
            .unwrap_or(1)
            .max(1);
        out.push_str(&format!(
            "{} | {}{}{}\n",
            gutter,
            " ".repeat(pad),
            "^".repeat(caret_len),
            label.map(|l| format!(" {l}")).unwrap_or_default()
        ));
    }
}

impl fmt::Display for EmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for EmError {}

impl From<String> for EmError {
    fn from(message: String) -> Self {
        EmError::new(message)
    }
}

#[allow(dead_code)]
pub type EmResult<T> = Result<T, EmError>;

/// Expand tabs to four columns so the source excerpt and the caret agree.
fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        if c == '\t' {
            out.push_str("    ");
        } else {
            out.push(c);
        }
    }
    out
}

/// Number of display columns before `col` (1-based character index), with
/// tabs expanded and common wide characters counted as two columns.
fn display_width_until(line: &str, col: usize) -> usize {
    let mut width = 0;
    for c in line.chars().take(col.saturating_sub(1)) {
        width += char_display_width(c);
    }
    width
}

fn char_display_width(c: char) -> usize {
    if c == '\t' {
        4
    } else if is_wide_char(c) {
        2
    } else {
        1
    }
}

/// A small, dependency-free approximation for East Asian wide characters.
fn is_wide_char(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD
    )
}

/// Compute a simple edit-distance based suggestion from a list of candidates.
pub fn suggest_closest<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a String>,
) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        let dist = levenshtein(name, candidate);
        if dist <= 2 {
            if best.is_none() || dist < best.unwrap().0 {
                best = Some((dist, candidate.as_str()));
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..=a.len() {
        dp[i][0] = i;
    }
    for j in 0..=b.len() {
        dp[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_includes_file_position_and_caret() {
        let err = EmError::name("Unknown variable 'agee'")
            .with_span(Span::new(2, 5))
            .with_hint("Did you mean 'age'?");
        let rendered = err.render("main.em", "age = 18\nsay(agee)\n");
        assert!(rendered.contains("main.em:2:5"));
        assert!(rendered.contains("2 | say(agee)"));
        assert!(rendered.contains("  |     ^"));
        assert!(rendered.contains("Did you mean 'age'?"));
    }

    #[test]
    fn render_expands_tabs_and_underlines_spans() {
        let err = EmError::syntax("bad token").with_span(Span::new(1, 2).with_len(3));
        let rendered = err.render("x.em", "\tfoo");
        assert!(rendered.contains("1 |     foo"));
        assert!(rendered.contains("  |     ^^^"));
    }

    #[test]
    fn notes_and_secondary_spans_are_rendered() {
        let err = EmError::name("duplicate name")
            .with_span(Span::new(2, 1))
            .with_secondary(Span::new(1, 1), "first defined here")
            .with_note("choose a different name");
        let rendered = err.render("x.em", "x = 1\nx = 2\n");
        assert!(rendered.contains("first defined here"));
        assert!(rendered.contains("= note: choose a different name"));
    }
}
