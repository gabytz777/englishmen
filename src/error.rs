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

    /// Build a span covering the inclusive column range `start..=end`.
    pub fn range(line: usize, start: usize, end: usize) -> Self {
        Self::new(line, start).with_end(end)
    }

    /// Mark the span as covering `len` characters starting at `col`.
    pub fn with_len(mut self, len: usize) -> Self {
        if len > 1 {
            self.end_col = Some(self.col + len - 1);
        }
        self
    }

    /// Mark the span as ending at an inclusive column.
    ///
    /// Accepts a plain column, a `(line, column)` pair (the line is checked
    /// against this span's line), or another span whose end should be reused.
    /// Out-of-line or backwards ends are ignored, as before.
    pub fn with_end(mut self, end: impl Into<SpanEnd>) -> Self {
        let end = end.into();
        if end.line.is_some_and(|line| line != self.line) {
            return self;
        }
        if end.col >= self.col {
            self.end_col = Some(end.col);
        }
        self
    }

    /// Explicit form of `with_end((line, end_col))`.
    pub fn with_end_line(self, end_line: usize, end_col: usize) -> Self {
        self.with_end((end_line, end_col))
    }

    /// Adopt the end column of `other` when both spans are on the same line.
    pub fn with_end_span(self, other: Span) -> Self {
        self.with_end(other)
    }

    /// The inclusive column where this span ends (the start column when the
    /// span covers a single character).
    pub fn end(&self) -> usize {
        self.end_col.unwrap_or(self.col)
    }

    /// Number of characters covered by the span (always at least one).
    pub fn len(&self) -> usize {
        self.end() - self.col + 1
    }

    /// True when `line` and `col` fall inside this span, inclusive.
    pub fn contains(&self, line: usize, col: usize) -> bool {
        line == self.line && col >= self.col && col <= self.end()
    }

    /// True when `col` falls inside this span on its line.
    pub fn contains_col(&self, col: usize) -> bool {
        self.contains(self.line, col)
    }

    /// True when two spans share at least one column on the same line.
    pub fn overlaps(&self, other: Span) -> bool {
        self.line == other.line && self.col <= other.end() && other.col <= self.end()
    }
}

/// Input accepted by [`Span::with_end`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanEnd {
    line: Option<usize>,
    col: usize,
}

impl From<usize> for SpanEnd {
    fn from(col: usize) -> Self {
        SpanEnd { line: None, col }
    }
}

impl From<(usize, usize)> for SpanEnd {
    fn from((line, col): (usize, usize)) -> Self {
        SpanEnd {
            line: Some(line),
            col,
        }
    }
}

impl From<Span> for SpanEnd {
    fn from(span: Span) -> Self {
        SpanEnd {
            line: Some(span.line),
            col: span.end(),
        }
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

    /// Stable diagnostic code for this category.
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::Syntax => "E001",
            ErrorKind::Name => "E002",
            ErrorKind::Type => "E003",
            ErrorKind::Index => "E004",
            ErrorKind::Import => "E005",
            ErrorKind::Io => "E006",
            ErrorKind::Arity => "E007",
            ErrorKind::Runtime | ErrorKind::User => "E008",
        }
    }
}

/// How seriously a diagnostic should be presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Severity {
    #[default]
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }

    pub fn is_error(self) -> bool {
        self == Severity::Error
    }

    pub fn is_warning(self) -> bool {
        self == Severity::Warning
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for Severity {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<Severity> for &str {
    fn eq(&self, other: &Severity) -> bool {
        *self == other.as_str()
    }
}

impl AsRef<str> for Severity {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::ops::Deref for Severity {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl From<Severity> for &'static str {
    fn from(severity: Severity) -> &'static str {
        severity.as_str()
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
    /// An optional first secondary location, used for notes such as
    /// "first defined here".
    pub secondary: Option<(Span, String)>,
    /// Additional secondary locations, in insertion order.
    pub secondary_labels: Vec<(Span, String)>,
    /// An optional, caller-supplied code that overrides the category default.
    pub code: Option<String>,
    /// An optional severity override (errors are the default).
    pub severity: Option<Severity>,
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
            secondary_labels: Vec::new(),
            code: None,
            severity: None,
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

    /// Attach a secondary location. Repeated calls add further labels, so a
    /// diagnostic can point at several related places in the source.
    pub fn with_secondary(mut self, span: Span, label: impl Into<String>) -> Self {
        let label = label.into();
        if self.secondary.is_none() {
            self.secondary = Some((span, label));
        } else {
            self.secondary_labels.push((span, label));
        }
        self
    }

    /// Alias for [`EmError::with_secondary`] for callers that think in labels.
    pub fn with_label(self, span: Span, label: impl Into<String>) -> Self {
        self.with_secondary(span, label)
    }

    /// All secondary locations: the first one (if any) followed by the rest.
    pub fn all_secondary_labels(&self) -> Vec<(Span, &str)> {
        self.secondary
            .iter()
            .chain(self.secondary_labels.iter())
            .map(|(span, label)| (*span, label.as_str()))
            .collect()
    }

    /// Only the additional labels added after the first one.
    pub fn secondary_labels(&self) -> &[(Span, String)] {
        &self.secondary_labels
    }

    /// Set a caller-defined code, overriding the code derived from the kind.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// The stable code derived from this error's category.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// The explicit code set with [`EmError::with_code`], if any.
    pub fn custom_code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Alias for [`EmError::custom_code`].
    pub fn code_override(&self) -> Option<&str> {
        self.custom_code()
    }

    /// The code that should be reported: the override when present, otherwise
    /// the stable category code.
    pub fn resolved_code(&self) -> String {
        self.custom_code()
            .map(str::to_owned)
            .unwrap_or_else(|| self.code().to_owned())
    }

    /// True for syntax diagnostics.
    pub fn is_syntax(&self) -> bool {
        self.kind == ErrorKind::Syntax
    }

    /// True for runtime diagnostics.
    pub fn is_runtime(&self) -> bool {
        self.kind == ErrorKind::Runtime
    }

    /// The presentation severity, defaulting to `Error`.
    pub fn severity(&self) -> Severity {
        self.severity.unwrap_or(Severity::Error)
    }

    /// Override the presentation severity.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = Some(severity);
        self
    }

    /// Serialize the essential fields as a compact JSON object.
    pub fn format_json(&self) -> String {
        let mut out = String::from("{\"message\":");
        push_json_string(&mut out, &self.message);
        out.push_str(",\"code\":");
        push_json_string(&mut out, &self.resolved_code());
        out.push_str(",\"span\":");
        match self.span {
            Some(span) => {
                out.push_str(&format!(
                    "{{\"line\":{},\"col\":{},\"end_col\":",
                    span.line, span.col
                ));
                match span.end_col {
                    Some(end) => out.push_str(&end.to_string()),
                    None => out.push_str("null"),
                }
                out.push('}');
            }
            None => out.push_str("null"),
        }
        out.push_str(",\"hint\":");
        match &self.hint {
            Some(hint) => push_json_string(&mut out, hint),
            None => out.push_str("null"),
        }
        out.push('}');
        out
    }

    /// Render a nicely formatted, source-aware error message.
    pub fn render(&self, filename: &str, source: &str) -> String {
        self.render_with_color(filename, source, false)
    }

    /// Like [`EmError::render`], but with ANSI colours for terminals.
    pub fn render_colored(&self, filename: &str, source: &str) -> String {
        self.render_with_color(filename, source, true)
    }

    fn render_with_color(&self, filename: &str, source: &str, color: bool) -> String {
        let mut out = String::new();
        if color {
            out.push_str("\x1b[1;31mError:\x1b[0m ");
            out.push_str(&self.message);
            out.push('\n');
        } else {
            out.push_str(&format!("Error: {}\n", self.message));
        }
        let file = self.file.as_deref().unwrap_or(filename);
        if let Some(span) = self.span {
            if color {
                out.push_str(&format!(
                    "  --> \x1b[36m{}:{}:{}\x1b[0m\n",
                    file, span.line, span.col
                ));
            } else {
                out.push_str(&format!("  --> {}:{}:{}\n", file, span.line, span.col));
            }
            Self::render_excerpt(&mut out, source, span, self.span_label(), color);
            if let Some((secondary_span, label)) = &self.secondary {
                Self::render_excerpt(&mut out, source, *secondary_span, Some(label.as_str()), color);
            }
            for (secondary_span, label) in &self.secondary_labels {
                Self::render_excerpt(&mut out, source, *secondary_span, Some(label.as_str()), color);
            }
        }
        if !self.notes.is_empty() {
            out.push('\n');
            for note in &self.notes {
                if color {
                    out.push_str("  = \x1b[32mnote\x1b[0m: ");
                } else {
                    out.push_str("  = note: ");
                }
                out.push_str(note);
                out.push('\n');
            }
        }
        if let Some(hint) = &self.hint {
            out.push('\n');
            if color {
                out.push_str("\x1b[33m");
                out.push_str(hint);
                out.push_str("\x1b[0m\n");
            } else {
                out.push_str(hint);
                out.push('\n');
            }
        }
        out
    }

    fn span_label(&self) -> Option<&str> {
        None
    }

    fn render_excerpt(
        out: &mut String,
        source: &str,
        span: Span,
        label: Option<&str>,
        color: bool,
    ) {
        // Compiled bytecode carries positions but not the source it came
        // from, so the excerpt is skipped rather than left blank.
        let Some(line_text) = source.lines().nth(span.line.saturating_sub(1)) else {
            return;
        };
        out.push('\n');
        let line_num = span.line.to_string();
        let gutter = " ".repeat(line_num.len());
        let expanded = expand_tabs(line_text);
        if color {
            out.push_str(&format!("\x1b[36m{}\x1b[0m | {}\n", line_num, expanded));
        } else {
            out.push_str(&format!("{} | {}\n", line_num, expanded));
        }
        let pad = display_width_until(line_text, span.col);
        let caret_len = span
            .end_col
            .map(|end| end.saturating_sub(span.col) + 1)
            .unwrap_or(1)
            .max(1);
        let carets = "^".repeat(caret_len);
        let carets = if color {
            format!("\x1b[1;31m{carets}\x1b[0m")
        } else {
            carets
        };
        let label_part = label
            .map(|l| {
                if color {
                    format!(" \x1b[35m{l}\x1b[0m")
                } else {
                    format!(" {l}")
                }
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "{} | {}{}{}\n",
            gutter,
            " ".repeat(pad),
            carets,
            label_part
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

/// A small collector for multiple diagnostics from a single pass.
#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    errors: Vec<EmError>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, error: EmError) -> &mut Self {
        self.errors.push(error);
        self
    }

    /// Add a plain runtime error without constructing an `EmError` by hand.
    pub fn error(&mut self, message: impl Into<String>) -> &mut Self {
        self.push(EmError::new(message))
    }

    pub fn extend(&mut self, errors: impl IntoIterator<Item = EmError>) -> &mut Self {
        self.errors.extend(errors);
        self
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn errors(&self) -> &[EmError] {
        &self.errors
    }

    pub fn into_errors(self) -> Vec<EmError> {
        self.errors
    }

    /// Render every collected error with the same source context.
    pub fn render_all(&self, filename: &str, source: &str) -> String {
        let mut out = String::new();
        for error in &self.errors {
            out.push_str(&error.render(filename, source));
        }
        out
    }
}

impl From<Diagnostics> for Vec<EmError> {
    fn from(diagnostics: Diagnostics) -> Self {
        diagnostics.into_errors()
    }
}

impl IntoIterator for Diagnostics {
    type Item = EmError;
    type IntoIter = std::vec::IntoIter<EmError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

/// Collect several diagnostics into a `Vec<EmError>`.
#[macro_export]
macro_rules! diagnostics {
    () => { Vec::new() };
    ($($error:expr),+ $(,)?) => {{
        let mut collected = Vec::new();
        $(collected.push($error);)+
        collected
    }};
}

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

/// Append a JSON string literal, escaping everything JSON requires.
fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c < '\u{20}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Compute a suggestion from a list of candidates.
///
/// Small typos keep the original edit-distance behaviour. If none is close
/// enough, prefix matches and case-insensitive variants are considered, so
/// partially typed or wrongly capitalised names still produce a useful hint.
pub fn suggest_closest<S: AsRef<str>>(
    name: &str,
    candidates: impl IntoIterator<Item = S>,
) -> Option<String> {
    let mut best: Option<(u8, usize, usize, String)> = None;
    for (index, candidate) in candidates.into_iter().enumerate() {
        let candidate = candidate.as_ref();
        let direct = levenshtein(name, candidate);
        let folded_name = name.to_lowercase();
        let folded_candidate = candidate.to_lowercase();
        let folded = levenshtein(&folded_name, &folded_candidate);

        // Rank 0-1 reproduce the original suggestions exactly (distance <= 2,
        // first candidate wins ties). Ranks 2+ are fallbacks only.
        let (rank, metric) = if direct == 0 {
            (0, 0)
        } else if direct <= 2 {
            (1, direct)
        } else if folded == 0 {
            (2, 0)
        } else if folded <= 2 {
            (3, folded)
        } else if candidate.starts_with(name) || name.starts_with(candidate) {
            (4, length_gap(name, candidate))
        } else if folded_candidate.starts_with(&folded_name)
            || folded_name.starts_with(&folded_candidate)
        {
            (5, length_gap(name, candidate))
        } else {
            continue;
        };

        let score = (rank, metric, index);
        let is_better = best.as_ref().map_or(true, |(b_rank, b_metric, b_index, _)| {
            score < (*b_rank, *b_metric, *b_index)
        });
        if is_better {
            best = Some((rank, metric, index, candidate.to_owned()));
        }
    }
    best.map(|(_, _, _, candidate)| candidate)
}

fn length_gap(a: &str, b: &str) -> usize {
    a.chars().count().abs_diff(b.chars().count())
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
        let rendered = err.render("main.enm", "age = 18\nsay(agee)\n");
        assert!(rendered.contains("main.enm:2:5"));
        assert!(rendered.contains("2 | say(agee)"));
        assert!(rendered.contains("  |     ^"));
        assert!(rendered.contains("Did you mean 'age'?"));
    }

    #[test]
    fn render_expands_tabs_and_underlines_spans() {
        let err = EmError::syntax("bad token").with_span(Span::new(1, 2).with_len(3));
        let rendered = err.render("x.enm", "\tfoo");
        assert!(rendered.contains("1 |     foo"));
        assert!(rendered.contains("  |     ^^^"));
    }

    #[test]
    fn notes_and_secondary_spans_are_rendered() {
        let err = EmError::name("duplicate name")
            .with_span(Span::new(2, 1))
            .with_secondary(Span::new(1, 1), "first defined here")
            .with_note("choose a different name");
        let rendered = err.render("x.enm", "x = 1\nx = 2\n");
        assert!(rendered.contains("first defined here"));
        assert!(rendered.contains("= note: choose a different name"));
    }

    #[test]
    fn stable_codes_are_derived_from_every_kind() {
        assert_eq!(EmError::syntax("s").code(), "E001");
        assert_eq!(EmError::name("n").code(), "E002");
        assert_eq!(EmError::type_error("t").code(), "E003");
        assert_eq!(EmError::index("i").code(), "E004");
        assert_eq!(EmError::import("m").code(), "E005");
        assert_eq!(EmError::io("o").code(), "E006");
        assert_eq!(EmError::arity("a").code(), "E007");
        assert_eq!(EmError::new("r").code(), "E008");
        assert_eq!(EmError::user("u").code(), "E008");
        for kind in [
            ErrorKind::Syntax,
            ErrorKind::Name,
            ErrorKind::Type,
            ErrorKind::Arity,
            ErrorKind::Index,
            ErrorKind::Io,
            ErrorKind::Import,
            ErrorKind::Runtime,
            ErrorKind::User,
        ] {
            assert_eq!(kind.code().len(), 4);
            assert!(kind.code().starts_with('E'));
        }
    }

    #[test]
    fn with_code_overrides_the_reported_code_without_changing_the_kind() {
        let err = EmError::syntax("bad").with_code("E042");
        assert_eq!(err.code(), "E001");
        assert_eq!(err.custom_code(), Some("E042"));
        assert_eq!(err.code_override(), Some("E042"));
        assert_eq!(err.resolved_code(), "E042");
        assert_eq!(err.kind, ErrorKind::Syntax);
        assert_eq!(EmError::new("plain").custom_code(), None);
        assert_eq!(EmError::new("plain").resolved_code(), "E008");
    }

    #[test]
    fn format_json_includes_message_code_span_and_hint() {
        let err = EmError::name("Unknown variable 'agee'")
            .with_span(Span::range(2, 5, 8))
            .with_hint("Did you mean 'age'?");
        let json = err.format_json();
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
        assert!(json.contains("\"message\":\"Unknown variable 'agee'\""));
        assert!(json.contains("\"code\":\"E002\""));
        assert!(json.contains("\"span\":{\"line\":2,\"col\":5,\"end_col\":8}"));
        assert!(json.contains("\"hint\":\"Did you mean 'age'?\""));
    }

    #[test]
    fn format_json_uses_null_for_missing_fields_and_escapes_text() {
        let err = EmError::new("quote: \" slash: \\ tab:\t").with_code("E900");
        let json = err.format_json();
        assert!(json.contains("\"span\":null"));
        assert!(json.contains("\"hint\":null"));
        assert!(json.contains("\"code\":\"E900\""));
        assert!(json.contains("\\\""));
        assert!(json.contains("\\\\"));
        assert!(json.contains("\\t"));
    }

    #[test]
    fn category_and_severity_helpers_classify_errors() {
        let syntax = EmError::syntax("bad");
        assert!(syntax.is_syntax());
        assert!(!syntax.is_runtime());
        assert_eq!(syntax.severity(), "error");
        assert_eq!(syntax.severity(), Severity::Error);

        let runtime = EmError::new("boom").with_severity(Severity::Warning);
        assert!(runtime.is_runtime());
        assert!(!runtime.is_syntax());
        assert_eq!(runtime.severity(), "warning");
        assert_eq!("warning", runtime.severity());
        assert_eq!(runtime.severity().as_str(), "warning");
        assert!(runtime.severity().is_warning());
        assert!(syntax.severity().is_error());
        assert!(runtime.severity().contains("warn"));
        let level = runtime.severity();
        let as_str: &str = level.as_ref();
        assert_eq!(as_str, "warning");
    }

    #[test]
    fn suggest_closest_keeps_edit_distance_suggestions() {
        let names = vec!["age".to_string(), "aged".to_string()];
        assert_eq!(suggest_closest("agee", names.iter()).as_deref(), Some("age"));
        let names = vec!["length".to_string()];
        assert_eq!(suggest_closest("lenght", names.iter()).as_deref(), Some("length"));
        let names = vec!["coat".to_string(), "catastrophe".to_string()];
        assert_eq!(suggest_closest("cat", names.iter()).as_deref(), Some("coat"));
    }

    #[test]
    fn suggest_closest_uses_case_insensitive_fallbacks() {
        let names = vec!["Print".to_string(), "println".to_string()];
        assert_eq!(suggest_closest("prinT", names.iter()).as_deref(), Some("Print"));
        let names = vec!["SAY".to_string()];
        assert_eq!(suggest_closest("sayy", names.iter()).as_deref(), Some("SAY"));
    }

    #[test]
    fn suggest_closest_uses_prefix_fallbacks_and_prefers_the_nearest() {
        let names = vec!["catapult".to_string(), "catalog".to_string()];
        assert_eq!(suggest_closest("cata", names.iter()).as_deref(), Some("catalog"));
        let names = vec!["uppercase".to_string()];
        assert_eq!(suggest_closest("UPPER", names.iter()).as_deref(), Some("uppercase"));
        let names = vec!["unrelated".to_string()];
        assert_eq!(suggest_closest("zzzzzzzz", names.iter()), None);
    }

    #[test]
    fn span_end_contains_and_overlaps_are_inclusive() {
        let span = Span::new(3, 4).with_end(7);
        assert_eq!(span.end(), 7);
        assert_eq!(span.len(), 4);
        assert!(span.contains(3, 4));
        assert!(span.contains(3, 7));
        assert!(!span.contains(3, 8));
        assert!(!span.contains(4, 5));
        assert!(span.contains_col(6));
        assert!(span.overlaps(Span::range(3, 7, 9)));
        assert!(!span.overlaps(Span::range(3, 8, 9)));
        assert!(!span.overlaps(Span::range(4, 5, 6)));

        let single = Span::new(1, 2);
        assert_eq!(single.end(), 2);
        assert_eq!(single.len(), 1);
        assert!(single.contains(1, 2));
    }

    #[test]
    fn with_end_accepts_columns_tuples_and_other_spans() {
        let span = Span::new(2, 5).with_end(9usize);
        assert_eq!(span.end_col, Some(9));
        assert_eq!(Span::new(2, 5).with_end((2, 8)).end_col, Some(8));
        assert_eq!(Span::new(2, 5).with_end((3, 8)).end_col, None);
        assert_eq!(Span::new(2, 5).with_end_line(2, 7).end_col, Some(7));
        let other = Span::new(2, 6).with_len(3);
        assert_eq!(Span::new(2, 5).with_end_span(other).end_col, Some(8));
        assert_eq!(Span::range(1, 3, 6).end_col, Some(6));
        assert_eq!(Span::new(1, 5).with_end(2).end_col, None);
    }

    #[test]
    fn render_keeps_single_label_output_unchanged_and_supports_many_labels() {
        let single = EmError::name("duplicate")
            .with_span(Span::new(2, 1))
            .with_secondary(Span::new(1, 1), "first defined here");
        assert_eq!(
            single.render("x.enm", "x = 1\nx = 2\n"),
            "Error: duplicate\n  --> x.enm:2:1\n\n2 | x = 2\n  | ^\n\n1 | x = 1\n  | ^ first defined here\n"
        );

        let multi = EmError::name("duplicate")
            .with_span(Span::new(3, 1))
            .with_secondary(Span::new(1, 1), "first defined here")
            .with_label(Span::new(2, 1), "also defined here")
            .with_secondary(Span::new(4, 1), "and here");
        let rendered = multi.render("x.enm", "x = 1\nx = 2\nx = 3\nx = 4\n");
        assert!(rendered.contains("first defined here"));
        assert!(rendered.contains("also defined here"));
        assert!(rendered.contains("and here"));
        assert_eq!(multi.all_secondary_labels().len(), 3);
        assert_eq!(multi.secondary_labels().len(), 2);
        assert_eq!(multi.secondary_labels()[0].1, "also defined here");
    }

    #[test]
    fn secondary_labels_are_not_rendered_without_a_primary_span() {
        let err = EmError::new("no position").with_secondary(Span::new(1, 1), "label");
        assert_eq!(err.render("x.enm", "x = 1\n"), "Error: no position\n");
    }

    #[test]
    fn render_colored_adds_ansi_codes_but_render_stays_plain() {
        let err = EmError::syntax("bad token")
            .with_span(Span::new(1, 2).with_len(3))
            .with_hint("try again")
            .with_note("details");
        let plain = err.render("x.enm", "foo\n");
        assert!(!plain.contains('\x1b'));
        let colored = err.render_colored("x.enm", "foo\n");
        assert!(colored.contains("\x1b[1;31mError:"));
        assert!(colored.contains("\x1b[36m"));
        assert!(colored.contains("\x1b[1;31m^^^\x1b[0m"));
        assert!(colored.contains("\x1b[32mnote\x1b[0m"));
        assert!(colored.contains("\x1b[33mtry again\x1b[0m"));
        assert!(colored.contains("bad token"));
    }

    #[test]
    fn diagnostics_macro_collects_errors() {
        let empty: Vec<EmError> = diagnostics!();
        assert!(empty.is_empty());

        let errors = diagnostics![
            EmError::syntax("first"),
            EmError::name("second"),
            EmError::new("third"),
        ];
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0].message, "first");
        assert_eq!(errors[1].message, "second");
        assert_eq!(errors[2].message, "third");
        assert_eq!(errors[0].code(), "E001");
    }

    #[test]
    fn diagnostics_builder_collects_and_renders_errors() {
        let mut diagnostics = Diagnostics::new();
        assert!(diagnostics.is_empty());
        diagnostics
            .error("one")
            .push(EmError::syntax("two"))
            .extend(diagnostics![EmError::name("three")]);
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(diagnostics.errors()[1].message, "two");
        assert!(diagnostics.render_all("x.enm", "").contains("Error: one"));
        let errors: Vec<EmError> = diagnostics.into();
        assert_eq!(errors.len(), 3);
        assert_eq!(
            Diagnostics::new().into_iter().count(),
            0,
            "empty builders iterate zero times"
        );
    }
}
