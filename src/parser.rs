use crate::ast::{expr_to_assignable, BinOp, Expr, Stmt, UnOp};
use crate::error::{EmError, Span};
use crate::lexer::{InterpPart, SpannedToken, Token};

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    /// Number of enclosing loops. A `break`/`continue` inside a function body
    /// does not see loops in the surrounding scope.
    loop_depth: usize,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Parser {
            tokens,
            pos: 0,
            loop_depth: 0,
        }
    }

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, EmError> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.is_at_end() {
            stmts.push(self.parse_statement()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    // ---- token helpers ----

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn advance(&mut self) -> SpannedToken {
        let tok = self.tokens[self.pos].clone();
        if !matches!(tok.token, Token::Eof) {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, token: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(token)
    }

    fn matches(&mut self, token: &Token) -> bool {
        if self.check(token) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: Token, context: &str) -> Result<SpannedToken, EmError> {
        if self.check(&token) {
            Ok(self.advance())
        } else {
            let span = self.peek_span();
            Err(EmError::syntax(format!(
                "Expected {} {context}, found {}",
                token.describe(),
                self.peek().describe()
            ))
            .with_span(span))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    fn end_of_statement(&mut self) -> Result<(), EmError> {
        match self.peek() {
            Token::Newline | Token::Eof => {
                self.skip_newlines();
                Ok(())
            }
            _ => {
                let span = self.peek_span();
                Err(EmError::syntax(format!(
                    "Expected end of line, found {}",
                    self.peek().describe()
                ))
                .with_span(span))
            }
        }
    }

    // ---- statements ----

    fn parse_statement(&mut self) -> Result<Stmt, EmError> {
        let span = self.peek_span();
        match self.peek() {
            Token::If => self.parse_if(),
            Token::Function => self.parse_function_def(),
            Token::Return => {
                self.advance();
                if matches!(self.peek(), Token::Newline | Token::Eof) {
                    self.end_of_statement()?;
                    Ok(Stmt::Return(None, span))
                } else {
                    let expr = self.parse_expression()?;
                    self.end_of_statement()?;
                    Ok(Stmt::Return(Some(expr), span))
                }
            }
            Token::Repeat => self.parse_repeat(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::Break => {
                self.advance();
                self.require_in_loop("break", span)?;
                self.end_of_statement()?;
                Ok(Stmt::Break(span))
            }
            Token::Continue => {
                self.advance();
                self.require_in_loop("continue", span)?;
                self.end_of_statement()?;
                Ok(Stmt::Continue(span))
            }
            Token::Import => {
                self.advance();
                let name = self.expect_identifier("after 'import'")?;
                self.end_of_statement()?;
                Ok(Stmt::Import(name, span))
            }
            _ => self.parse_assign_or_expr(),
        }
    }

    fn require_in_loop(&self, keyword: &str, span: Span) -> Result<(), EmError> {
        if self.loop_depth == 0 {
            return Err(EmError::syntax(format!(
                "'{keyword}' can only be used inside a loop"
            ))
            .with_span(span)
            .with_hint("Place it inside a 'while', 'repeat', or 'for ... in' loop."));
        }
        Ok(())
    }

    fn expect_identifier(&mut self, context: &str) -> Result<String, EmError> {
        match self.peek().clone() {
            Token::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            other => {
                let span = self.peek_span();
                Err(EmError::syntax(format!(
                    "Expected identifier {context}, found {}",
                    other.describe()
                ))
                .with_span(span))
            }
        }
    }

    fn parse_block_until(&mut self, terminators: &[Token]) -> Result<Vec<Stmt>, EmError> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !terminators.iter().any(|t| self.check(t)) && !self.is_at_end() {
            stmts.push(self.parse_statement()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn parse_if(&mut self) -> Result<Stmt, EmError> {
        let span = self.peek_span();
        self.advance(); // if
        let mut branches = Vec::new();
        let cond = self.parse_expression()?;
        self.end_of_statement()?;
        let body = self.parse_block_until(&[Token::Else, Token::End])?;
        branches.push((cond, body));

        let mut else_body: Option<Vec<Stmt>> = None;
        loop {
            if self.check(&Token::Else) {
                self.advance();
                if self.check(&Token::If) {
                    self.advance();
                    let cond = self.parse_expression()?;
                    self.end_of_statement()?;
                    let body = self.parse_block_until(&[Token::Else, Token::End])?;
                    branches.push((cond, body));
                    continue;
                } else {
                    self.end_of_statement()?;
                    let body = self.parse_block_until(&[Token::End])?;
                    else_body = Some(body);
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(Token::End, "to close 'if'")?;
        self.end_of_statement()?;
        Ok(Stmt::If(branches, else_body, span))
    }

    fn parse_function_def(&mut self) -> Result<Stmt, EmError> {
        let span = self.peek_span();
        self.advance(); // function
        let name = self.expect_identifier("for function name")?;
        self.expect(Token::LParen, "after function name")?;
        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                params.push(self.expect_identifier("in parameter list")?);
                if self.matches(&Token::Comma) {
                    continue;
                }
                break;
            }
        }
        self.expect(Token::RParen, "to close parameter list")?;
        self.end_of_statement()?;
        // Loops in the enclosing scope do not apply inside a function.
        let outer_depth = self.loop_depth;
        self.loop_depth = 0;
        let body = self.parse_block_until(&[Token::End])?;
        self.loop_depth = outer_depth;
        self.expect(Token::End, "to close function")?;
        self.end_of_statement()?;
        Ok(Stmt::FunctionDef(name, params, body, span))
    }

    fn parse_repeat(&mut self) -> Result<Stmt, EmError> {
        let span = self.peek_span();
        self.advance(); // repeat
        let count = self.parse_expression()?;
        self.end_of_statement()?;
        self.loop_depth += 1;
        let body = self.parse_block_until(&[Token::End])?;
        self.loop_depth -= 1;
        self.expect(Token::End, "to close 'repeat'")?;
        self.end_of_statement()?;
        Ok(Stmt::Repeat(count, body, span))
    }

    fn parse_while(&mut self) -> Result<Stmt, EmError> {
        let span = self.peek_span();
        self.advance(); // while
        let cond = self.parse_expression()?;
        self.end_of_statement()?;
        self.loop_depth += 1;
        let body = self.parse_block_until(&[Token::End])?;
        self.loop_depth -= 1;
        self.expect(Token::End, "to close 'while'")?;
        self.end_of_statement()?;
        Ok(Stmt::While(cond, body, span))
    }

    fn parse_for(&mut self) -> Result<Stmt, EmError> {
        let span = self.peek_span();
        self.advance(); // for
        let var = self.expect_identifier("after 'for'")?;
        self.expect(Token::In, "after loop variable")?;
        let iterable = self.parse_expression()?;
        self.end_of_statement()?;
        self.loop_depth += 1;
        let body = self.parse_block_until(&[Token::End])?;
        self.loop_depth -= 1;
        self.expect(Token::End, "to close 'for'")?;
        self.end_of_statement()?;
        Ok(Stmt::ForIn(var, iterable, body, span))
    }

    fn parse_assign_or_expr(&mut self) -> Result<Stmt, EmError> {
        let span = self.peek_span();
        let expr = self.parse_expression()?;
        if self.matches(&Token::Assign) {
            let value = self.parse_expression()?;
            self.end_of_statement()?;
            let assignable = expr_to_assignable(expr, span)?;
            return Ok(Stmt::Assign(assignable, value, span));
        }
        if let Token::CompoundAssign(op) = self.peek().clone() {
            let span = self.peek_span();
            self.advance();
            let value = self.parse_expression()?;
            self.end_of_statement()?;
            let assignable = expr_to_assignable(expr, span)?;
            return Ok(Stmt::CompoundAssign(assignable, op, value, span));
        }
        self.end_of_statement()?;
        Ok(Stmt::ExprStmt(expr, span))
    }

    // ---- expressions (precedence climbing) ----

    fn parse_expression(&mut self) -> Result<Expr, EmError> {
        // Ternary has the lowest precedence and is right-associative.
        let cond = self.parse_coalesce()?;
        if self.check(&Token::Question) {
            let span = self.peek_span();
            self.advance();
            let when_true = self.parse_expression()?;
            self.expect(Token::Colon, "after the true branch of '?'")?;
            let when_false = self.parse_expression()?;
            return Ok(Expr::Ternary(
                Box::new(cond),
                Box::new(when_true),
                Box::new(when_false),
                span,
            ));
        }
        Ok(cond)
    }

    fn parse_coalesce(&mut self) -> Result<Expr, EmError> {
        let left = self.parse_or()?;
        if self.check(&Token::NullCoalesce) {
            let span = self.peek_span();
            self.advance();
            // `??` is right-associative: `a ?? b ?? c` is `a ?? (b ?? c)`.
            let right = self.parse_coalesce()?;
            return Ok(Expr::Binary(Box::new(left), BinOp::Coalesce, Box::new(right), span));
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_bit_or()?;
        loop {
            let op = match self.peek() {
                Token::Or | Token::OrElse => BinOp::Or,
                _ => break,
            };
            let span = self.peek_span();
            self.advance();
            let right = self.parse_bit_or()?;
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_bit_or(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_bit_xor()?;
        while self.check(&Token::BitOr) {
            let span = self.peek_span();
            self.advance();
            let right = self.parse_bit_xor()?;
            left = Expr::Binary(Box::new(left), BinOp::BitOr, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_bit_and()?;
        while self.check(&Token::BitXor) {
            let span = self.peek_span();
            self.advance();
            let right = self.parse_bit_and()?;
            left = Expr::Binary(Box::new(left), BinOp::BitXor, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_shift()?;
        while self.check(&Token::BitAnd) {
            let span = self.peek_span();
            self.advance();
            let right = self.parse_shift()?;
            left = Expr::Binary(Box::new(left), BinOp::BitAnd, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_and()?;
        loop {
            let op = match self.peek() {
                Token::Shl => BinOp::Shl,
                Token::Shr => BinOp::Shr,
                _ => break,
            };
            let span = self.peek_span();
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_not()?;
        loop {
            let op = match self.peek() {
                Token::And | Token::AndAlso => BinOp::And,
                _ => break,
            };
            let span = self.peek_span();
            self.advance();
            let right = self.parse_not()?;
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, EmError> {
        if matches!(self.peek(), Token::Not | Token::Bang) {
            let span = self.peek_span();
            self.advance();
            let operand = self.parse_not()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(operand), span));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::Eq => BinOp::Eq,
                Token::NotEq => BinOp::NotEq,
                Token::Gt => BinOp::Gt,
                Token::Lt => BinOp::Lt,
                Token::GtEq => BinOp::GtEq,
                Token::LtEq => BinOp::LtEq,
                _ => break,
            };
            let span = self.peek_span();
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            let span = self.peek_span();
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, EmError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                Token::FloorDiv | Token::Div => BinOp::FloorDiv,
                _ => break,
            };
            let span = self.peek_span();
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, EmError> {
        if matches!(self.peek(), Token::Minus | Token::Not | Token::Bang | Token::BitNot) {
            let span = self.peek_span();
            let op = match self.peek() {
                Token::Minus => UnOp::Neg,
                Token::BitNot => UnOp::BitNot,
                _ => UnOp::Not,
            };
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary(op, Box::new(operand), span));
        }
        self.parse_power()
    }

    /// Exponentiation binds tighter than unary minus and is right-associative:
    /// `-2 ** 2` is `-(2 ** 2)` and `2 ** 3 ** 2` is `2 ** (3 ** 2)`.
    fn parse_power(&mut self) -> Result<Expr, EmError> {
        let left = self.parse_postfix()?;
        if self.check(&Token::Pow) {
            let span = self.peek_span();
            self.advance();
            let right = self.parse_unary()?;
            return Ok(Expr::Binary(
                Box::new(left),
                BinOp::Pow,
                Box::new(right),
                span,
            ));
        }
        Ok(left)
    }

    fn parse_postfix(&mut self) -> Result<Expr, EmError> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.matches(&Token::Dot) {
                let name = self.expect_identifier("after '.'")?;
                if self.check(&Token::LParen) {
                    let args = self.parse_call_args()?;
                    expr = Expr::MethodCall(Box::new(expr), name, args);
                } else {
                    expr = Expr::PropertyAccess(Box::new(expr), name);
                }
            } else if self.matches(&Token::LBracket) {
                let index = self.parse_expression()?;
                self.expect(Token::RBracket, "to close index expression")?;
                expr = Expr::Index(Box::new(expr), Box::new(index));
            } else if self.check(&Token::LParen) {
                let args = self.parse_call_args()?;
                expr = Expr::Call(Box::new(expr), args);
            } else if matches!(self.peek(), Token::PlusPlus | Token::MinusMinus) {
                let span = self.peek_span();
                let increment = matches!(self.peek(), Token::PlusPlus);
                // Validate here so an invalid target fails at parse time in
                // both backends, not only when the expression is executed.
                expr_to_assignable(expr.clone(), span)?;
                self.advance();
                expr = Expr::Postfix(Box::new(expr), increment, span);
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, EmError> {
        self.expect(Token::LParen, "to start argument list")?;
        let mut args = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                args.push(self.parse_expression()?);
                if self.matches(&Token::Comma) {
                    continue;
                }
                break;
            }
        }
        self.expect(Token::RParen, "to close argument list")?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, EmError> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::String(s) => {
                self.advance();
                Ok(Expr::String(s))
            }
            Token::Interpolated(parts) => {
                self.advance();
                self.parse_interpolated(parts, span)
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Null)
            }
            Token::Identifier(name) => {
                self.advance();
                Ok(Expr::Variable(name))
            }
            Token::LParen => {
                self.advance();
                self.skip_newlines();
                let expr = self.parse_expression()?;
                self.skip_newlines();
                self.expect(Token::RParen, "to close parenthesized expression")?;
                Ok(expr)
            }
            Token::LBracket => self.parse_array_literal(),
            Token::LBrace => self.parse_object_literal(),
            other => {
                Err(EmError::syntax(format!("Unexpected token {}", other.describe())).with_span(span))
            }
        }
    }

    /// Lower an interpolated string into ordinary `+` expressions. This keeps
    /// the compiler, VM and interpreter unchanged and guarantees that an
    /// interpolated value is always converted to text.
    fn parse_interpolated(
        &mut self,
        parts: Vec<InterpPart>,
        outer_span: Span,
    ) -> Result<Expr, EmError> {
        let has_expression = parts
            .iter()
            .any(|part| matches!(part, InterpPart::Expr(_)));
        let mut acc = if has_expression {
            Some(Expr::String(String::new()))
        } else {
            None
        };

        for part in parts {
            let (expr, span) = match part {
                InterpPart::Literal(text) => (Expr::String(text), outer_span),
                InterpPart::Expr(tokens) => {
                    let span = tokens
                        .iter()
                        .find(|token| !matches!(token.token, Token::Newline | Token::Eof))
                        .map(|token| token.span)
                        .unwrap_or(outer_span);
                    let mut parser = Parser::new(tokens);
                    parser.skip_newlines();
                    let expr = parser.parse_expression()?;
                    parser.skip_newlines();
                    if !parser.is_at_end() {
                        return Err(EmError::syntax(format!(
                            "Unexpected token {} in interpolation",
                            parser.peek().describe()
                        ))
                        .with_span(parser.peek_span())
                        .with_hint("Each '{...}' must contain one complete expression."));
                    }
                    (expr, span)
                }
            };
            acc = Some(match acc {
                Some(left) => Expr::Binary(Box::new(left), BinOp::Add, Box::new(expr), span),
                None => expr,
            });
        }

        Ok(acc.unwrap_or(Expr::String(String::new())))
    }

    fn parse_array_literal(&mut self) -> Result<Expr, EmError> {
        self.expect(Token::LBracket, "to start array literal")?;
        self.skip_newlines();
        let mut items = Vec::new();
        if !self.check(&Token::RBracket) {
            loop {
                self.skip_newlines();
                items.push(self.parse_expression()?);
                self.skip_newlines();
                if self.matches(&Token::Comma) {
                    self.skip_newlines();
                    continue;
                }
                break;
            }
        }
        self.skip_newlines();
        self.expect(Token::RBracket, "to close array literal")?;
        Ok(Expr::Array(items))
    }

    fn parse_object_literal(&mut self) -> Result<Expr, EmError> {
        self.expect(Token::LBrace, "to start object literal")?;
        self.skip_newlines();
        let mut fields = Vec::new();
        if !self.check(&Token::RBrace) {
            loop {
                self.skip_newlines();
                let key = self.expect_identifier("as object key")?;
                self.expect(Token::Colon, "after object key")?;
                self.skip_newlines();
                let value = self.parse_expression()?;
                fields.push((key, value));
                self.skip_newlines();
                if self.matches(&Token::Comma) {
                    self.skip_newlines();
                    continue;
                }
                break;
            }
        }
        self.skip_newlines();
        self.expect(Token::RBrace, "to close object literal")?;
        Ok(Expr::Object(fields))
    }
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Vec<Stmt>, EmError> {
    Parser::new(tokens).parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Assignable;
    use crate::lexer::tokenize;

    fn parse_src(src: &str) -> Vec<Stmt> {
        parse(tokenize(src).unwrap()).unwrap()
    }

    #[test]
    fn variable_assignment() {
        let stmts = parse_src("age = 18\n");
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::Assign(Assignable::Variable(name), Expr::Number(n), _) => {
                assert_eq!(name, "age");
                assert_eq!(*n, 18.0);
            }
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn operator_precedence() {
        let stmts = parse_src("result = 2 + 3 * 4\n");
        match &stmts[0] {
            Stmt::Assign(_, Expr::Binary(left, BinOp::Add, right, _), _) => {
                assert!(matches!(**left, Expr::Number(2.0)));
                assert!(matches!(**right, Expr::Binary(_, BinOp::Mul, _, _)));
            }
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn interpolation_lowers_to_addition() {
        let stmts = parse_src("say(\"Hello, {name}!\")\n");
        let Stmt::ExprStmt(Expr::Call(_, args), _) = &stmts[0] else {
            panic!("unexpected stmt: {:?}", stmts[0]);
        };
        let Expr::Binary(left, BinOp::Add, right, _) = &args[0] else {
            panic!("expected concatenation, got {:?}", args[0]);
        };
        assert!(matches!(**left, Expr::Binary(_, BinOp::Add, _, _)));
        assert!(matches!(**right, Expr::String(ref s) if s == "!"));
    }

    #[test]
    fn invalid_interpolation_expression_is_reported() {
        let err = parse(tokenize("x = \"Hi {1 +}\"\n").unwrap()).unwrap_err();
        assert!(err.message.contains("Unexpected token"));
        assert_eq!(err.span.unwrap().line, 1);
    }

    #[test]
    fn empty_interpolation_is_reported() {
        let err = tokenize("x = \"Hi {}\"\n").unwrap_err();
        assert_eq!(err.message, "Empty interpolation");
    }

    #[test]
    fn property_access_ast_shape() {
        let stmts = parse_src("say(person.age)\n");
        match &stmts[0] {
            Stmt::ExprStmt(Expr::Call(_, args), _) => match &args[0] {
                Expr::PropertyAccess(base, name) => {
                    assert!(matches!(**base, Expr::Variable(ref v) if v == "person"));
                    assert_eq!(name, "age");
                }
                other => panic!("unexpected arg: {other:?}"),
            },
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn object_literal() {
        let stmts = parse_src("person = { name: \"Alex\", age: 18 }\n");
        match &stmts[0] {
            Stmt::Assign(_, Expr::Object(fields), _) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "name");
                assert_eq!(fields[1].0, "age");
            }
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn array_literal() {
        let stmts = parse_src("people = [\"Alex\", \"Sam\"]\n");
        match &stmts[0] {
            Stmt::Assign(_, Expr::Array(items), _) => assert_eq!(items.len(), 2),
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn if_else_chain() {
        let stmts = parse_src("if age >= 18\n  say(\"a\")\nelse if age >= 13\n  say(\"b\")\nelse\n  say(\"c\")\nend\n");
        match &stmts[0] {
            Stmt::If(branches, else_body, _) => {
                assert_eq!(branches.len(), 2);
                assert!(else_body.is_some());
            }
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn function_definition() {
        let stmts = parse_src("function add(a, b)\n  return a + b\nend\n");
        match &stmts[0] {
            Stmt::FunctionDef(name, params, body, _) => {
                assert_eq!(name, "add");
                assert_eq!(params, &vec!["a".to_string(), "b".to_string()]);
                assert_eq!(body.len(), 1);
            }
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn repeat_and_for_loops() {
        let stmts = parse_src("repeat 3\n  say(\"hi\")\nend\nfor p in people\n  say(p)\nend\n");
        assert!(matches!(stmts[0], Stmt::Repeat(..)));
        assert!(matches!(stmts[1], Stmt::ForIn(..)));
    }

    #[test]
    fn import_statement() {
        let stmts = parse_src("import math\n");
        match &stmts[0] {
            Stmt::Import(name, _) => assert_eq!(name, "math"),
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn while_break_and_continue() {
        let stmts = parse_src("while n < 10\n  if n == 3\n    continue\n  end\n  if n == 8\n    break\n  end\n  n = n + 1\nend\n");
        let Stmt::While(_, body, _) = &stmts[0] else {
            panic!("expected while loop");
        };
        assert_eq!(body.len(), 3);
        let Stmt::If(branches, _, _) = &body[0] else { panic!("expected if") };
        assert!(matches!(branches[0].1[0], Stmt::Continue(_)));
        let Stmt::If(branches, _, _) = &body[1] else { panic!("expected if") };
        assert!(matches!(branches[0].1[0], Stmt::Break(_)));
    }

    #[test]
    fn break_and_continue_outside_loops_are_rejected() {
        for src in ["break\n", "continue\n"] {
            let err = parse(tokenize(src).unwrap()).unwrap_err();
            assert!(err.message.contains("only be used inside a loop"));
        }
        // A function body does not inherit the loop depth of its definition.
        let err = parse(tokenize("repeat 1\nfunction f()\n  break\nend\nend\n").unwrap()).unwrap_err();
        assert!(err.message.contains("only be used inside a loop"));
    }

    #[test]
    fn compound_assignments_cover_every_target() {
        let stmts = parse_src("x += 2\no.a -= 3\na[0] *= 4\nx **= 2\nx //= 2\nx &= 1\n");
        assert!(matches!(
            &stmts[0],
            Stmt::CompoundAssign(Assignable::Variable(name), BinOp::Add, Expr::Number(2.0), _)
            if name == "x"
        ));
        assert!(matches!(
            &stmts[1],
            Stmt::CompoundAssign(Assignable::Property(..), BinOp::Sub, _, _)
        ));
        assert!(matches!(
            &stmts[2],
            Stmt::CompoundAssign(Assignable::Index(..), BinOp::Mul, _, _)
        ));
        assert!(matches!(&stmts[3], Stmt::CompoundAssign(_, BinOp::Pow, _, _)));
        assert!(matches!(&stmts[4], Stmt::CompoundAssign(_, BinOp::FloorDiv, _, _)));
        assert!(matches!(&stmts[5], Stmt::CompoundAssign(_, BinOp::BitAnd, _, _)));
    }

    #[test]
    fn postfix_operators_are_expressions() {
        let stmts = parse_src("x++\no.a--\na[0]++\n");
        assert!(matches!(&stmts[0], Stmt::ExprStmt(Expr::Postfix(_, true, _), _)));
        assert!(matches!(&stmts[1], Stmt::ExprStmt(Expr::Postfix(_, false, _), _)));
        assert!(matches!(&stmts[2], Stmt::ExprStmt(Expr::Postfix(_, true, _), _)));
        let err = parse(tokenize("(1 + 2)++\n").unwrap()).unwrap_err();
        assert!(err.message.contains("Invalid assignment target"));
    }

    #[test]
    fn exponent_is_right_associative_and_binds_tighter_than_unary_minus() {
        let stmts = parse_src("x = 2 ** 3 ** 2\ny = -2 ** 2\n");
        let Stmt::Assign(_, Expr::Binary(_, BinOp::Pow, right, _), _) = &stmts[0] else {
            panic!("expected exponent");
        };
        assert!(matches!(**right, Expr::Binary(_, BinOp::Pow, _, _)));
        let Stmt::Assign(_, Expr::Unary(UnOp::Neg, inner, _), _) = &stmts[1] else {
            panic!("expected negation outside exponent");
        };
        assert!(matches!(**inner, Expr::Binary(_, BinOp::Pow, _, _)));
    }

    #[test]
    fn floor_division_and_bitwise_precedence() {
        let stmts = parse_src("x = 7 // 2 + 1\ny = 1 | 2 & 3 ^ 4\nz = ~1 << 2\n");
        let Stmt::Assign(_, Expr::Binary(_, BinOp::Add, _, _), _) = &stmts[0] else {
            panic!("floor division binds tighter than addition");
        };
        let Stmt::Assign(_, Expr::Binary(_, BinOp::BitOr, _, _), _) = &stmts[1] else {
            panic!("bitwise or is the widest bitwise operator");
        };
        let Stmt::Assign(_, Expr::Binary(_, BinOp::Shl, _, _), _) = &stmts[2] else {
            panic!("shifts bind looser than unary complement");
        };
    }

    #[test]
    fn ternary_and_coalesce_ast_shapes() {
        let stmts = parse_src("x = true ? 1 : 2\ny = a ?? b ?? c\n");
        let Stmt::Assign(_, Expr::Ternary(_, _, _, _), _) = &stmts[0] else {
            panic!("expected ternary");
        };
        let Stmt::Assign(_, Expr::Binary(_, BinOp::Coalesce, right, _), _) = &stmts[1] else {
            panic!("expected coalesce");
        };
        assert!(matches!(**right, Expr::Binary(_, BinOp::Coalesce, _, _)));
    }

    /// Compare ASTs without source positions: `&&` and `and` occupy a
    /// different number of columns, but must build the same expression.
    fn without_spans(stmts: &[Stmt]) -> String {
        let text = format!("{stmts:?}");
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(index) = rest.find("Span {") {
            out.push_str(&rest[..index]);
            rest = &rest[index..];
            let end = rest.find('}').expect("a closing brace for Span").saturating_add(1);
            rest = &rest[end..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn symbol_aliases_match_word_operators() {
        let left = parse_src("x = a and b or not c\n");
        let right = parse_src("x = a && b || !c\n");
        assert_eq!(without_spans(&left), without_spans(&right));
    }
}
