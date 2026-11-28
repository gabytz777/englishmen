use crate::error::{EmError, Span};

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Variable(String),
    PropertyAccess(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Call(Box<Expr>, Vec<Expr>),
    MethodCall(Box<Expr>, String, Vec<Expr>),
    Binary(Box<Expr>, BinOp, Box<Expr>, Span),
    Unary(UnOp, Box<Expr>, Span),
    /// A lazy `condition ? whenTrue : whenFalse` expression.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>, Span),
    /// Postfix `++` / `--` on an assignable expression. The bool is `true`
    /// for `++` and `false` for `--`.
    Postfix(Box<Expr>, bool, Span),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// Floor division (`//` or `div`).
    FloorDiv,
    /// Exponentiation (`**`), right-associative.
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    NotEq,
    Gt,
    Lt,
    GtEq,
    LtEq,
    And,
    Or,
    /// Null-coalescing (`??`).
    Coalesce,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    /// Bitwise complement (`~`).
    BitNot,
}

/// The left-hand side of an assignment.
#[derive(Debug, Clone)]
pub enum Assignable {
    Variable(String),
    Property(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
}

/// Convert a parsed expression into an assignment target. Shared by the parser
/// and both backends so `x++` and `(x + 1)++` produce the same error.
pub fn expr_to_assignable(expr: Expr, span: Span) -> Result<Assignable, EmError> {
    match expr {
        Expr::Variable(name) => Ok(Assignable::Variable(name)),
        Expr::PropertyAccess(base, name) => Ok(Assignable::Property(base, name)),
        Expr::Index(base, index) => Ok(Assignable::Index(base, index)),
        _ => Err(EmError::syntax("Invalid assignment target")
            .with_span(span)
            .with_hint("Only variables, properties, and array indexes can be assigned to.")),
    }
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign(Assignable, Expr, Span),
    /// `target += value` (and friends). The operator is a `BinOp`.
    CompoundAssign(Assignable, BinOp, Expr, Span),
    ExprStmt(Expr, Span),
    If(Vec<(Expr, Vec<Stmt>)>, Option<Vec<Stmt>>, Span),
    FunctionDef(String, Vec<String>, Vec<Stmt>, Span),
    Return(Option<Expr>, Span),
    Repeat(Expr, Vec<Stmt>, Span),
    While(Expr, Vec<Stmt>, Span),
    ForIn(String, Expr, Vec<Stmt>, Span),
    Break(Span),
    Continue(Span),
    Import(String, Span),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_targets_accept_variables_properties_and_indexes() {
        assert!(matches!(
            expr_to_assignable(Expr::Variable("x".into()), Span::default()),
            Ok(Assignable::Variable(_))
        ));
        assert!(matches!(
            expr_to_assignable(
                Expr::PropertyAccess(Box::new(Expr::Variable("o".into())), "a".into()),
                Span::default()
            ),
            Ok(Assignable::Property(..))
        ));
        assert!(matches!(
            expr_to_assignable(
                Expr::Index(Box::new(Expr::Variable("a".into())), Box::new(Expr::Number(0.0))),
                Span::default()
            ),
            Ok(Assignable::Index(..))
        ));
    }

    #[test]
    fn invalid_assignment_target_has_a_stable_message() {
        let err = expr_to_assignable(Expr::Number(1.0), Span::new(3, 7)).unwrap_err();
        assert_eq!(err.message, "Invalid assignment target");
        assert_eq!(err.span.unwrap().line, 3);
    }
}
