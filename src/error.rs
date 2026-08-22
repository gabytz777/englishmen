use std::fmt;

/// A position in an Englishmen source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(line: usize, col: usize) -> Self {
        Span { line, col }
    }
}

/// A rich, user-facing error produced anywhere in the pipeline.
#[derive(Debug, Clone)]
pub struct EmError {
    pub message: String,
    pub span: Option<Span>,
    pub hint: Option<String>,
}

impl EmError {
    pub fn new(message: impl Into<String>) -> Self {
        EmError {
            message: message.into(),
            span: None,
            hint: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Render a nicely formatted, source-aware error message.
    pub fn render(&self, filename: &str, source: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("Error: {}\n", self.message));
        if let Some(span) = self.span {
            out.push_str(&format!("  --> {}:{}:{}\n", filename, span.line, span.col));
            // Compiled bytecode carries positions but not the source it came
            // from, so the excerpt is skipped rather than left blank.
            if let Some(line_text) = source.lines().nth(span.line.saturating_sub(1)) {
                out.push('\n');
                let line_num = span.line.to_string();
                let gutter = " ".repeat(line_num.len());
                out.push_str(&format!("{} | {}\n", line_num, line_text));
                let caret_pad = " ".repeat(span.col.saturating_sub(1));
                out.push_str(&format!("{} | {}^\n", gutter, caret_pad));
            }
        }
        if let Some(hint) = &self.hint {
            out.push('\n');
            out.push_str(hint);
            out.push('\n');
        }
        out
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
