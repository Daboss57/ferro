// AST node definitions — the tree structure that represents a Ferro program.

pub mod pretty;

use crate::error::Span;

/// A complete Ferro program: a list of top-level items (functions).
#[derive(Debug)]
pub struct Program {
    pub items: Vec<Function>,
}

/// A function definition: `fn name(params) -> return_type { body }`
#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>, // None means no return type (void)
    pub body: Block,
    pub span: Span,
}

/// A function parameter: `name: type`
#[derive(Debug)]
pub struct Param {
    pub name: String,
    pub type_name: String,
    pub span: Span,
}

/// A block of statements: `{ stmt1; stmt2; ... }`
#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

/// A statement — things that DO something but don't produce a value.
#[derive(Debug)]
pub enum Stmt {
    /// `let x: i64 = expr;` or `let mut x: i64 = expr;`
    Let {
        name: String,
        mutable: bool,
        type_name: Option<String>,
        value: Expr,
        span: Span,
    },
    /// `x = expr;`
    Assign {
        name: String,
        value: Expr,
        span: Span,
    },
    /// `return expr;`
    Return {
        value: Option<Expr>,
        span: Span,
    },
    /// An expression used as a statement (e.g., a function call: `print(x);`)
    Expr {
        expr: Expr,
        span: Span,
    },
    /// `if condition { ... } else { ... }`
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    /// `while condition { ... }`
    While {
        condition: Expr,
        body: Block,
        span: Span,
    },
}

/// An expression — things that PRODUCE a value.
#[derive(Debug)]
pub enum Expr {
    /// Integer literal: `42`
    IntLit {
        value: i64,
        span: Span,
    },
    /// Boolean literal: `true` or `false`
    BoolLit {
        value: bool,
        span: Span,
    },
    /// String literal: `"hello"`
    StringLit {
        value: String,
        span: Span,
    },
    /// Variable reference: `x`
    Ident {
        name: String,
        span: Span,
    },
    /// Binary operation: `a + b`, `x == y`
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// Unary operation: `-x`, `!flag`
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    /// Function call: `add(1, 2)`
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,        // +
    Sub,        // -
    Mul,        // *
    Div,        // /
    Eq,         // ==
    Neq,        // !=
    Lt,         // <
    Gt,         // >
    Lte,        // <=
    Gte,        // >=
    And,        // &&
    Or,         // ||
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,   // -x
    Not,   // !x
}
