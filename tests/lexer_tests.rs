use ferro::lexer::Lexer;
use ferro::lexer::token::TokenKind;

/// Helper: lex source code and return just the token kinds (ignoring spans).
fn lex(source: &str) -> Vec<TokenKind> {
    let mut lexer = Lexer::new(source);
    lexer
        .tokenize()
        .expect("lexer error")
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

#[test]
fn test_empty_input() {
    assert_eq!(lex(""), vec![TokenKind::Eof]);
}

#[test]
fn test_integer_literals() {
    assert_eq!(lex("42"), vec![TokenKind::Int(42), TokenKind::Eof]);
    assert_eq!(lex("0"), vec![TokenKind::Int(0), TokenKind::Eof]);
    assert_eq!(lex("1000"), vec![TokenKind::Int(1000), TokenKind::Eof]);
}

#[test]
fn test_string_literal() {
    assert_eq!(
        lex(r#""hello""#),
        vec![TokenKind::StringLit("hello".to_string()), TokenKind::Eof]
    );
}

#[test]
fn test_string_escape_sequences() {
    assert_eq!(
        lex(r#""hello\nworld""#),
        vec![
            TokenKind::StringLit("hello\nworld".to_string()),
            TokenKind::Eof,
        ]
    );
    assert_eq!(
        lex(r#""tab\there""#),
        vec![
            TokenKind::StringLit("tab\there".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_identifiers() {
    assert_eq!(
        lex("x foo bar_baz _test abc123"),
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Ident("foo".into()),
            TokenKind::Ident("bar_baz".into()),
            TokenKind::Ident("_test".into()),
            TokenKind::Ident("abc123".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_keywords() {
    assert_eq!(
        lex("fn let mut if else while return true false"),
        vec![
            TokenKind::Fn,
            TokenKind::Let,
            TokenKind::Mut,
            TokenKind::If,
            TokenKind::Else,
            TokenKind::While,
            TokenKind::Return,
            TokenKind::True,
            TokenKind::False,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_keyword_prefix_is_identifier() {
    // "letter" starts with "let" but should be an identifier, not a keyword
    assert_eq!(
        lex("letter iffy"),
        vec![
            TokenKind::Ident("letter".into()),
            TokenKind::Ident("iffy".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_operators() {
    assert_eq!(
        lex("+ - * / = == != < > <= >= ! && ||"),
        vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Equals,
            TokenKind::EqualEqual,
            TokenKind::BangEqual,
            TokenKind::Less,
            TokenKind::Greater,
            TokenKind::LessEqual,
            TokenKind::GreaterEqual,
            TokenKind::Bang,
            TokenKind::AmpAmp,
            TokenKind::PipePipe,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_arrow() {
    assert_eq!(
        lex("->"),
        vec![TokenKind::Arrow, TokenKind::Eof]
    );
}

#[test]
fn test_pipe_arrow() {
    assert_eq!(
        lex("|>"),
        vec![TokenKind::PipeArrow, TokenKind::Eof]
    );
}

#[test]
fn test_pipe_arrow_in_chain() {
    assert_eq!(
        lex("x |> f |> g"),
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::PipeArrow,
            TokenKind::Ident("f".into()),
            TokenKind::PipeArrow,
            TokenKind::Ident("g".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_punctuation() {
    assert_eq!(
        lex("( ) { } ; : ,"),
        vec![
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Semicolon,
            TokenKind::Colon,
            TokenKind::Comma,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_line_comments_are_skipped() {
    assert_eq!(
        lex("42 // this is a comment\n7"),
        vec![TokenKind::Int(42), TokenKind::Int(7), TokenKind::Eof]
    );
}

#[test]
fn test_full_let_statement() {
    assert_eq!(
        lex("let x: i64 = 42;"),
        vec![
            TokenKind::Let,
            TokenKind::Ident("x".into()),
            TokenKind::Colon,
            TokenKind::Ident("i64".into()),
            TokenKind::Equals,
            TokenKind::Int(42),
            TokenKind::Semicolon,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_function_definition() {
    let source = "fn add(a: i64, b: i64) -> i64 { return a + b; }";
    assert_eq!(
        lex(source),
        vec![
            TokenKind::Fn,
            TokenKind::Ident("add".into()),
            TokenKind::LParen,
            TokenKind::Ident("a".into()),
            TokenKind::Colon,
            TokenKind::Ident("i64".into()),
            TokenKind::Comma,
            TokenKind::Ident("b".into()),
            TokenKind::Colon,
            TokenKind::Ident("i64".into()),
            TokenKind::RParen,
            TokenKind::Arrow,
            TokenKind::Ident("i64".into()),
            TokenKind::LBrace,
            TokenKind::Return,
            TokenKind::Ident("a".into()),
            TokenKind::Plus,
            TokenKind::Ident("b".into()),
            TokenKind::Semicolon,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_if_else() {
    let source = "if x == 0 { true } else { false }";
    assert_eq!(
        lex(source),
        vec![
            TokenKind::If,
            TokenKind::Ident("x".into()),
            TokenKind::EqualEqual,
            TokenKind::Int(0),
            TokenKind::LBrace,
            TokenKind::True,
            TokenKind::RBrace,
            TokenKind::Else,
            TokenKind::LBrace,
            TokenKind::False,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_source_positions() {
    let mut lexer = Lexer::new("let x");
    let tokens = lexer.tokenize().unwrap();

    // "let" starts at line 1, column 1
    assert_eq!(tokens[0].span.start.line, 1);
    assert_eq!(tokens[0].span.start.column, 1);

    // "x" starts at line 1, column 5
    assert_eq!(tokens[1].span.start.line, 1);
    assert_eq!(tokens[1].span.start.column, 5);
}

#[test]
fn test_multiline_positions() {
    let mut lexer = Lexer::new("a\nb");
    let tokens = lexer.tokenize().unwrap();

    // "a" is on line 1
    assert_eq!(tokens[0].span.start.line, 1);
    // "b" is on line 2
    assert_eq!(tokens[1].span.start.line, 2);
    assert_eq!(tokens[1].span.start.column, 1);
}

#[test]
fn test_unexpected_character_error() {
    let mut lexer = Lexer::new("@");
    let err = lexer.tokenize().unwrap_err();
    assert!(err.message.contains("unexpected character '@'"));
}

#[test]
fn test_unterminated_string_error() {
    let mut lexer = Lexer::new("\"hello");
    let err = lexer.tokenize().unwrap_err();
    assert!(err.message.contains("unterminated string"));
}


// ── Import & Priv keyword tests ────────────────────────

#[test]
fn test_import_keyword() {
    let mut lexer = Lexer::new("import");
    let tokens = lexer.tokenize().unwrap();
    assert_eq!(tokens[0].kind, TokenKind::Import);
}

#[test]
fn test_priv_keyword() {
    let mut lexer = Lexer::new("priv");
    let tokens = lexer.tokenize().unwrap();
    assert_eq!(tokens[0].kind, TokenKind::Priv);
}