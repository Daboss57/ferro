use crate::error::Span;

/// Every possible token in the Ferro language.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),           // 42, 0, 1000
    StringLit(String),  // "hello world"

    // Identifier (variable names, function names, type names)
    Ident(String),      // x, myVar, add, i64

    // Keywords
    Fn,       // fn
    Let,      // let
    Mut,      // mut
    If,       // if
    Else,     // else
    While,    // while
    For,      // for
    In,       // in
    Break,    // break
    Continue, // continue
    Match,    // match
    Enum,     // enum
    Struct,   // struct
    Defer,    // defer
    Try,      // try
    Fail,     // fail
    Return,   // return
    True,     // true
    False,    // false

    // Operators
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Percent,      // %
    Equals,       // =
    EqualEqual,   // ==
    BangEqual,    // !=
    Less,         // <
    Greater,      // >
    LessEqual,    // <=
    GreaterEqual, // >=
    Bang,         // !
    AmpAmp,       // &&
    PipePipe,     // ||
    Arrow,        // ->
    FatArrow,     // =>
    PipeArrow,    // |>
    DotDot,       // ..
    Dot,          // .
    ColonColon,   // ::

    // Punctuation
    LParen,    // (
    RParen,    // )
    LBrace,    // {
    RBrace,    // }
    LBracket,  // [
    RBracket,  // ]
    Semicolon, // ;
    Colon,     // :
    Comma,     // ,

    // Special
    Eof,
}

/// A token with its kind and source location.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Look up whether an identifier is actually a keyword.
pub fn lookup_keyword(word: &str) -> Option<TokenKind> {
    match word {
        "fn"       => Some(TokenKind::Fn),
        "let"      => Some(TokenKind::Let),
        "mut"      => Some(TokenKind::Mut),
        "if"       => Some(TokenKind::If),
        "else"     => Some(TokenKind::Else),
        "while"    => Some(TokenKind::While),
        "for"      => Some(TokenKind::For),
        "in"       => Some(TokenKind::In),
        "break"    => Some(TokenKind::Break),
        "continue" => Some(TokenKind::Continue),
        "match"    => Some(TokenKind::Match),
        "enum"     => Some(TokenKind::Enum),
        "struct"   => Some(TokenKind::Struct),
        "defer"    => Some(TokenKind::Defer),
        "try"      => Some(TokenKind::Try),
        "fail"     => Some(TokenKind::Fail),
        "return"   => Some(TokenKind::Return),
        "true"     => Some(TokenKind::True),
        "false"    => Some(TokenKind::False),
        _        => None,
    }
}
