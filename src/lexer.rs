use crate::error::{EmError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    String(String),
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
            Token::Identifier(s) => format!("identifier '{s}'"),
            Token::Eof => "end of file".to_string(),
            Token::Newline => "newline".to_string(),
            other => format!("'{other:?}'"),
        }
    }
}

#[derive(Debug, Clone)]
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
                        return Err(EmError::new("Unterminated comment")
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
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => break,
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some(other) => s.push(other),
                    None => {
                        return Err(EmError::new("Unterminated string literal")
                            .with_span(start)
                            .with_hint("Strings must be closed with a matching '\"'."))
                    }
                },
                Some(c) => s.push(c),
                None => {
                    return Err(EmError::new("Unterminated string literal")
                        .with_span(start)
                        .with_hint("Strings must be closed with a matching '\"'."))
                }
            }
        }
        Ok(Token::String(s))
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
                    return Err(EmError::new("Unexpected character '!'")
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
                return Err(EmError::new(format!("Unexpected character '{other}'")).with_span(span))
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
