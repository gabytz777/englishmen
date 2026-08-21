use crate::error::Span;

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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Gt,
    Lt,
    GtEq,
    LtEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

/// The left-hand side of an assignment.
#[derive(Debug, Clone)]
pub enum Assignable {
    Variable(String),
    Property(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign(Assignable, Expr, Span),
    ExprStmt(Expr, Span),
    If(Vec<(Expr, Vec<Stmt>)>, Option<Vec<Stmt>>, Span),
    FunctionDef(String, Vec<String>, Vec<Stmt>, Span),
    Return(Option<Expr>, Span),
    Repeat(Expr, Vec<Stmt>, Span),
    ForIn(String, Expr, Vec<Stmt>, Span),
    Import(String, Span),
}
