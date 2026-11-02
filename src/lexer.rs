use crate::error::{EmError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    String(String),
    /// A string containing `{expression}` parts. The lexer keeps literal text
    /// and tokenized expressions separate so the parser can report precise
    /// positions for bad interpolations.
    Interpolated(Vec<InterpPart>),
    Identifier(String),
    True,
    False,
    Null,

    // Keywords
    If,
    Else,
    End,
    Function,
    Return,
    Repeat,
    For,
    In,
    Import,
    And,
    Or,
    Not,

    // Symbols
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Eq,
    NotEq,
    Gt,
    Lt,
    GtEq,
    LtEq,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Dot,
    Comma,
    Colon,
    Newline,
    Eof,
}

impl Token {
    pub fn describe(&self) -> String {
        match self {
            Token::Number(n) => format!("number '{n}'"),
            Token::String(s) => format!("string \"{s}\""),
            Token::Interpolated(_) => "an interpolated string".to_string(),
            Token::Identifier(s) => format!("identifier '{s}'"),
            Token::Eof => "end of file".to_string(),
            Token::Newline => "newline".to_string(),
            other => format!("'{other:?}'"),
        }
    }
}

/// One piece of an interpolated string.
#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    Literal(String),
    Expr(Vec<SpannedToken>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn span(&self) -> Span {
        Span::new(self.line, self.col)
    }

    pub fn tokenize(mut self) -> Result<Vec<SpannedToken>, EmError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments()?;
            let span = self.span();
            let Some(c) = self.peek() else {
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    span,
                });
                break;
            };

            if c == '\n' {
                self.advance();
                tokens.push(SpannedToken {
                    token: Token::Newline,
                    span,
                });
                continue;
            }

            if c.is_ascii_digit() {
                tokens.push(SpannedToken {
                    token: self.read_number(),
                    span,
                });
                continue;
            }

            if c == '"' {
                tokens.push(SpannedToken {
                    token: self.read_string(span)?,
                    span,
                });
                continue;
            }

            if c.is_alphabetic() || c == '_' {
                tokens.push(SpannedToken {
                    token: self.read_identifier(),
                    span,
                });
                continue;
            }

            let token = self.read_symbol(span)?;
            tokens.push(SpannedToken { token, span });
        }
        Ok(tokens)
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), EmError> {
        loop {
            match self.peek() {
                Some(c) if c == ' ' || c == '\t' || c == '\r' => {
                    self.advance();
                }
                Some('$') if self.peek_at(1) == Some('-') => {
                    let start_span = self.span();
                    self.advance(); // $
                    self.advance(); // -
                    let mut closed = false;
                    while let Some(c) = self.peek() {
                        if c == '-' && self.peek_at(1) == Some('$') {
                            self.advance();
                            self.advance();
                            closed = true;
                            break;
                        }
                        self.advance();
                    }
                    if !closed {
                        return Err(EmError::syntax("Unterminated comment")
                            .with_span(start_span)
                            .with_hint("Comments must be closed with '-$'."));
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if self.peek() == Some('.') && self.peek_at(1).map(|c| c.is_ascii_digit()).unwrap_or(false)
        {
            s.push('.');
            self.advance();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
        }
        Token::Number(s.parse().unwrap_or(0.0))
    }

    fn read_string(&mut self, start: Span) -> Result<Token, EmError> {
        self.advance(); // opening quote
        let mut literal = String::new();
        let mut parts: Vec<InterpPart> = Vec::new();
        loop {
            match self.advance() {
                Some('"') => break,
                Some('\\') => match self.advance() {
                    Some('n') => literal.push('\n'),
                    Some('t') => literal.push('\t'),
                    Some('"') => literal.push('"'),
                    Some('\\') => literal.push('\\'),
                    Some('{') => literal.push('{'),
                    Some('}') => literal.push('}'),
                    Some(other) => literal.push(other),
                    None => {
                        return Err(EmError::syntax("Unterminated string literal")
                            .with_span(start)
                            .with_hint("Strings must be closed with a matching '\"'."))
                    }
                },
                Some('{') => {
                    // `{{` is an escaped literal brace.
                    if self.peek() == Some('{') {
                        self.advance();
                        literal.push('{');
                        continue;
                    }
                    if !literal.is_empty() {
                        parts.push(InterpPart::Literal(std::mem::take(&mut literal)));
                    }
                    let expr_start = self.span();
                    let tokens = self.read_interpolation(start, expr_start)?;
                    if tokens.is_empty() || matches!(tokens.first().map(|t| &t.token), Some(Token::Eof))
                    {
                        return Err(EmError::syntax("Empty interpolation")
                            .with_span(expr_start)
                            .with_hint("Put an expression between '{' and '}', or write '{{' for a literal brace."));
                    }
                    parts.push(InterpPart::Expr(tokens));
                }
                Some('}') => {
                    // `}}` is an escaped literal brace.
                    if self.peek() == Some('}') {
                        self.advance();
                    }
                    literal.push('}');
                }
                Some(c) => literal.push(c),
                None => {
                    return Err(EmError::syntax("Unterminated string literal")
                        .with_span(start)
                        .with_hint("Strings must be closed with a matching '\"'."))
                }
            }
        }
        if parts.is_empty() {
            return Ok(Token::String(literal));
        }
        if !literal.is_empty() {
            parts.push(InterpPart::Literal(literal));
        }
        Ok(Token::Interpolated(parts))
    }

    /// Consume the body of a `{...}` interpolation and return tokens with
    /// positions adjusted to the surrounding source file.
    fn read_interpolation(
        &mut self,
        string_start: Span,
        expr_start: Span,
    ) -> Result<Vec<SpannedToken>, EmError> {
        let mut raw = String::new();
        let mut depth = 0usize;
        loop {
            match self.peek() {
                None => {
                    return Err(EmError::syntax("Unclosed interpolation")
                        .with_span(expr_start)
                        .with_hint("Add a closing '}' at the end of the expression."))
                }
                Some('\\') => {
                    // Escapes inside an interpolation belong to the
                    // expression's source; copy them so a `\"` does not start
                    // a nested string by accident.
                    raw.push('\\');
                    self.advance();
                    if let Some(c) = self.advance() {
                        raw.push(c);
                    }
                }
                Some('"') => {
                    raw.push('"');
                    self.advance();
                    loop {
                        match self.advance() {
                            Some('"') => {
                                raw.push('"');
                                break;
                            }
                            Some('\\') => {
                                raw.push('\\');
                                if let Some(c) = self.advance() {
                                    raw.push(c);
                                }
                            }
                            Some(c) => raw.push(c),
                            None => {
                                return Err(EmError::syntax("Unterminated string literal")
                                    .with_span(string_start)
                                    .with_hint(
                                        "Strings must be closed with a matching '\"'.",
                                    ))
                            }
                        }
                    }
                }
                Some('$') if self.peek_at(1) == Some('-') => {
                    raw.push('$');
                    raw.push('-');
                    self.advance();
                    self.advance();
                    while let Some(c) = self.advance() {
                        raw.push(c);
                        if c == '-' && self.peek() == Some('$') {
                            raw.push('$');
                            self.advance();
                            break;
                        }
                    }
                }
                Some('{') => {
                    depth += 1;
                    raw.push('{');
                    self.advance();
                }
                Some('}') => {
                    self.advance();
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    raw.push('}');
                }
                Some(c) => {
                    raw.push(c);
                    self.advance();
                }
            }
        }

        let tokens = Lexer::new(&raw).tokenize().map_err(|mut err| {
            if let Some(span) = err.span {
                err.span = Some(offset_span(span, expr_start));
            }
            err
        })?;
        Ok(tokens
            .into_iter()
            .map(|mut token| {
                token.span = offset_span(token.span, expr_start);
                token
            })
            .collect())
    }

    fn read_identifier(&mut self) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        match s.as_str() {
            "if" => Token::If,
            "else" => Token::Else,
            "end" => Token::End,
            "function" => Token::Function,
            "return" => Token::Return,
            "repeat" => Token::Repeat,
            "for" => Token::For,
            "in" => Token::In,
            "import" => Token::Import,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "true" => Token::True,
            "false" => Token::False,
            "null" => Token::Null,
            _ => Token::Identifier(s),
        }
    }

    fn read_symbol(&mut self, span: Span) -> Result<Token, EmError> {
        let c = self.advance().unwrap();
        let token = match c {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '.' => Token::Dot,
            ',' => Token::Comma,
            ':' => Token::Colon,
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Eq
                } else {
                    Token::Assign
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::NotEq
                } else {
                    return Err(EmError::syntax("Unexpected character '!'")
                        .with_span(span)
                        .with_hint("Did you mean '!=' or 'not'?"));
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::GtEq
                } else {
                    Token::Gt
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token::LtEq
                } else {
                    Token::Lt
                }
            }
            other => {
                return Err(EmError::syntax(format!("Unexpected character '{other}'")).with_span(span))
            }
        };
        Ok(token)
    }
}

pub fn tokenize(source: &str) -> Result<Vec<SpannedToken>, EmError> {
    // Notepad and several other Windows editors save UTF-8 with a byte order
    // mark. It carries no meaning for us, and reporting it as a stray
    // character on line 1 is baffling when the line looks perfectly fine.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    Lexer::new(source).tokenize()
}

/// Shift a fragment span (line 1, col 1 based) into the surrounding source.
fn offset_span(span: Span, base: Span) -> Span {
    let line = base.line + span.line.saturating_sub(1);
    let col = if span.line == 1 {
        base.col + span.col.saturating_sub(1)
    } else {
        span.col
    };
    Span {
        line,
        col,
        end_col: span.end_col.map(|end| {
            if span.line == 1 {
                base.col + end.saturating_sub(1)
            } else {
                end
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(source: &str) -> Vec<Token> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn a_byte_order_mark_is_ignored() {
        // Files saved as "UTF-8 with BOM" (Notepad's default) must lex
        // exactly like the same file without one.
        assert_eq!(toks("\u{feff}say(1)\n"), toks("say(1)\n"));
    }

    #[test]
    fn interpolated_strings_split_literals_and_expressions() {
        let tokens = tokenize("\"Hi {name}!\"\n").unwrap();
        let Token::Interpolated(parts) = &tokens[0].token else {
            panic!("expected an interpolated string");
        };
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], InterpPart::Literal(s) if s == "Hi "));
        assert!(matches!(&parts[1], InterpPart::Expr(tokens) if matches!(tokens[0].token, Token::Identifier(_))));
        assert!(matches!(&parts[2], InterpPart::Literal(s) if s == "!"));
    }

    #[test]
    fn escaped_braces_stay_literal() {
        assert_eq!(
            toks("\"{{name}} and \\{x\\}\"\n"),
            vec![Token::String("{name} and {x}".to_string()), Token::Newline, Token::Eof]
        );
    }

    #[test]
    fn interpolation_fragment_spans_point_into_the_file() {
        let tokens = tokenize("say(\"Hi {name}!\")\n").unwrap();
        let Token::Interpolated(parts) = &tokens[2].token else {
            panic!("expected an interpolated string");
        };
        let InterpPart::Expr(expr) = &parts[1] else {
            panic!("expected an expression part");
        };
        assert_eq!(expr[0].span.line, 1);
        assert_eq!(expr[0].span.col, 10);
    }

    #[test]
    fn a_byte_order_mark_does_not_shift_positions() {
        let with_bom = tokenize("\u{feff}say(1)\n").unwrap();
        let without = tokenize("say(1)\n").unwrap();
        let spans: Vec<_> = with_bom.iter().map(|t| t.span).collect();
        let expected: Vec<_> = without.iter().map(|t| t.span).collect();
        assert_eq!(spans, expected);
    }

    #[test]
    fn keywords() {
        assert_eq!(
            toks("if else end function return repeat for in import and or not true false null"),
            vec![
                Token::If,
                Token::Else,
                Token::End,
                Token::Function,
                Token::Return,
                Token::Repeat,
                Token::For,
                Token::In,
                Token::Import,
                Token::And,
                Token::Or,
                Token::Not,
                Token::True,
                Token::False,
                Token::Null,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn identifiers() {
        assert_eq!(
            toks("age person_name x1"),
            vec![
                Token::Identifier("age".to_string()),
                Token::Identifier("person_name".to_string()),
                Token::Identifier("x1".to_string()),
                Token::Eof
            ]
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(
            toks("18 3.14 0"),
            vec![
                Token::Number(18.0),
                Token::Number(3.14),
                Token::Number(0.0),
                Token::Eof
            ]
        );
    }

    #[test]
    fn strings() {
        assert_eq!(
            toks(r#""hello" "with \"escape\"""#),
            vec![
                Token::String("hello".to_string()),
                Token::String("with \"escape\"".to_string()),
                Token::Eof
            ]
        );
    }

    #[test]
    fn operators() {
        assert_eq!(
            toks("+ - * / % = == != > < >= <="),
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Percent,
                Token::Assign,
                Token::Eq,
                Token::NotEq,
                Token::Gt,
                Token::Lt,
                Token::GtEq,
                Token::LtEq,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn single_line_comment_is_skipped() {
        assert_eq!(
            toks("$- comment -$ age"),
            vec![Token::Identifier("age".to_string()), Token::Eof]
        );
    }

    #[test]
    fn multiline_comment_is_skipped() {
        let source = "$-\nline one\nline two\n-$ age";
        assert_eq!(
            toks(source),
            vec![Token::Identifier("age".to_string()), Token::Eof]
        );
    }

    #[test]
    fn unterminated_comment_errors() {
        let err = tokenize("$- never closed").unwrap_err();
        assert!(err.message.contains("Unterminated comment"));
    }

    #[test]
    fn unterminated_string_errors() {
        let err = tokenize("\"never closed").unwrap_err();
        assert!(err.message.contains("Unterminated string"));
    }
}
