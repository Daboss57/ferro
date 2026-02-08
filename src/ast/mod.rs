// AST node definitions — the tree structure that represents a Ferro program.

pub mod pretty;

use crate::error::Span;

/// A complete Ferro program: a list of top-level items.
#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
    pub enums: Vec<EnumDef>,
}

/// An enum definition: `enum Color { Red, Green, Blue }`
#[derive(Debug)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<String>,
    pub span: Span,
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
    /// `arr[index] = expr;`
    IndexAssign {
        object: String,
        index: Expr,
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
    /// Implicit return: last expression in a block without semicolon.
    /// `fn add(a: i64, b: i64) -> i64 { a + b }`
    TailExpr {
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
    /// `for var in start..end { ... }`
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Block,
        span: Span,
    },
    /// `break;`
    Break {
        span: Span,
    },
    /// `continue;`
    Continue {
        span: Span,
    },
    /// `match subject { pattern => { body } ... }`
    Match {
        subject: Expr,
        arms: Vec<MatchArm>,
        span: Span,
    },
}

/// A single arm of a match statement.
#[derive(Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Block,
    pub span: Span,
}

/// A pattern in a match arm.
#[derive(Debug)]
pub enum Pattern {
    /// Integer literal: `42`
    IntLit(i64, Span),
    /// Boolean literal: `true` / `false`
    BoolLit(bool, Span),
    /// Wildcard: `_`
    Wildcard(Span),
    /// Enum variant: `Color::Red`
    EnumVariant(String, String, Span),
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
    /// Array literal: `[1, 2, 3]`
    ArrayLit {
        elements: Vec<Expr>,
        span: Span,
    },
    /// Index expression: `arr[i]`
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// Enum variant expression: `Color::Red`
    EnumVariant {
        enum_name: String,
        variant: String,
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
    Mod,        // %
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
