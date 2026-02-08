use ferro::ast::pretty::pretty_print;
use ferro::ast::*;
use ferro::lexer::Lexer;
use ferro::parser::Parser;

/// Helper: parse source code into an AST program.
fn parse(source: &str) -> Program {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().expect("lexer error");
    let mut parser = Parser::new(tokens);
    parser.parse_program().expect("parser error")
}

/// Helper: parse and pretty-print, trimming whitespace for comparison.
fn parse_and_print(source: &str) -> String {
    let program = parse(source);
    pretty_print(&program)
}

// ── Expressions ─────────────────────────────────────────

#[test]
fn test_integer_literal() {
    let prog = parse("fn main() { 42; }");
    if let Stmt::Expr { expr: Expr::IntLit { value, .. }, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(*value, 42);
    } else {
        panic!("expected int literal");
    }
}

#[test]
fn test_boolean_literals() {
    let prog = parse("fn main() { true; false; }");
    if let Stmt::Expr { expr: Expr::BoolLit { value, .. }, .. } = &prog.functions[0].body.stmts[0] {
        assert!(*value);
    } else {
        panic!("expected true");
    }
    if let Stmt::Expr { expr: Expr::BoolLit { value, .. }, .. } = &prog.functions[0].body.stmts[1] {
        assert!(!*value);
    } else {
        panic!("expected false");
    }
}

#[test]
fn test_string_literal() {
    let prog = parse(r#"fn main() { "hello"; }"#);
    if let Stmt::Expr { expr: Expr::StringLit { value, .. }, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(value, "hello");
    } else {
        panic!("expected string literal");
    }
}

#[test]
fn test_binary_ops_precedence() {
    // * binds tighter than +, so: 1 + (2 * 3)
    let output = parse_and_print("fn main() { 1 + 2 * 3; }");
    assert!(output.contains("(1 + (2 * 3))"));
}

#[test]
fn test_left_associativity() {
    // 1 - 2 - 3 should be (1 - 2) - 3
    let output = parse_and_print("fn main() { 1 - 2 - 3; }");
    assert!(output.contains("((1 - 2) - 3)"));
}

#[test]
fn test_comparison_and_logical() {
    let output = parse_and_print("fn main() { a < b && c == d; }");
    // && binds looser than < and ==
    assert!(output.contains("((a < b) && (c == d))"));
}

#[test]
fn test_unary_negation() {
    let output = parse_and_print("fn main() { -42; }");
    assert!(output.contains("-42"));
}

#[test]
fn test_unary_not() {
    let output = parse_and_print("fn main() { !flag; }");
    assert!(output.contains("!flag"));
}

#[test]
fn test_parenthesized_expr() {
    // Parens override precedence: (1 + 2) * 3
    let output = parse_and_print("fn main() { (1 + 2) * 3; }");
    assert!(output.contains("((1 + 2) * 3)"));
}

// ── Function calls ──────────────────────────────────────

#[test]
fn test_function_call_no_args() {
    let prog = parse("fn main() { foo(); }");
    if let Stmt::Expr { expr: Expr::Call { name, args, .. }, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "foo");
        assert!(args.is_empty());
    } else {
        panic!("expected function call");
    }
}

#[test]
fn test_function_call_with_args() {
    let prog = parse("fn main() { add(1, 2); }");
    if let Stmt::Expr { expr: Expr::Call { name, args, .. }, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "add");
        assert_eq!(args.len(), 2);
    } else {
        panic!("expected function call");
    }
}

// ── Let statements ──────────────────────────────────────

#[test]
fn test_let_with_type() {
    let prog = parse("fn main() { let x: i64 = 42; }");
    if let Stmt::Let { name, mutable, type_name, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "x");
        assert!(!mutable);
        assert_eq!(type_name.as_deref(), Some("i64"));
    } else {
        panic!("expected let statement");
    }
}

#[test]
fn test_let_mut() {
    let prog = parse("fn main() { let mut y: i64 = 0; }");
    if let Stmt::Let { name, mutable, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "y");
        assert!(mutable);
    } else {
        panic!("expected mutable let");
    }
}

#[test]
fn test_let_without_type() {
    let prog = parse("fn main() { let z = 10; }");
    if let Stmt::Let { name, type_name, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "z");
        assert!(type_name.is_none());
    } else {
        panic!("expected let without type");
    }
}

// ── Assignment ──────────────────────────────────────────

#[test]
fn test_assignment() {
    let prog = parse("fn main() { x = 99; }");
    if let Stmt::Assign { name, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "x");
    } else {
        panic!("expected assignment");
    }
}

// ── Return ──────────────────────────────────────────────

#[test]
fn test_return_with_value() {
    let prog = parse("fn main() { return 42; }");
    if let Stmt::Return { value: Some(Expr::IntLit { value, .. }), .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(*value, 42);
    } else {
        panic!("expected return with value");
    }
}

#[test]
fn test_return_void() {
    let prog = parse("fn main() { return; }");
    if let Stmt::Return { value: None, .. } = &prog.functions[0].body.stmts[0] {
        // good
    } else {
        panic!("expected void return");
    }
}

// ── If/Else ─────────────────────────────────────────────

#[test]
fn test_if_without_else() {
    let prog = parse("fn main() { if x { 1; } }");
    if let Stmt::If { else_block: None, .. } = &prog.functions[0].body.stmts[0] {
        // good
    } else {
        panic!("expected if without else");
    }
}

#[test]
fn test_if_with_else() {
    let prog = parse("fn main() { if x { 1; } else { 2; } }");
    if let Stmt::If { else_block: Some(_), .. } = &prog.functions[0].body.stmts[0] {
        // good
    } else {
        panic!("expected if with else");
    }
}

// ── While ───────────────────────────────────────────────

#[test]
fn test_while_loop() {
    let prog = parse("fn main() { while x { 1; } }");
    if let Stmt::While { .. } = &prog.functions[0].body.stmts[0] {
        // good
    } else {
        panic!("expected while loop");
    }
}

// ── Functions ───────────────────────────────────────────

#[test]
fn test_function_with_return_type() {
    let prog = parse("fn add(a: i64, b: i64) -> i64 { return a; }");
    assert_eq!(prog.functions[0].name, "add");
    assert_eq!(prog.functions[0].params.len(), 2);
    assert_eq!(prog.functions[0].params[0].name, "a");
    assert_eq!(prog.functions[0].params[0].type_name, "i64");
    assert_eq!(prog.functions[0].params[1].name, "b");
    assert_eq!(prog.functions[0].return_type.as_deref(), Some("i64"));
}

#[test]
fn test_function_no_params_no_return() {
    let prog = parse("fn main() { 1; }");
    assert_eq!(prog.functions[0].name, "main");
    assert!(prog.functions[0].params.is_empty());
    assert!(prog.functions[0].return_type.is_none());
}

#[test]
fn test_multiple_functions() {
    let prog = parse("fn foo() { 1; } fn bar() { 2; }");
    assert_eq!(prog.functions.len(), 2);
    assert_eq!(prog.functions[0].name, "foo");
    assert_eq!(prog.functions[1].name, "bar");
}

// ── Full programs ───────────────────────────────────────

#[test]
fn test_full_program_pretty_print() {
    let source = r#"
fn add(a: i64, b: i64) -> i64 {
    return a + b;
}

fn main() {
    let x: i64 = 42;
    let mut i: i64 = 0;
    while i < 10 {
        i = i + 1;
    }
    if x == 42 {
        add(x, i);
    } else {
        add(0, 0);
    }
}
"#;
    let output = parse_and_print(source);
    // Verify the pretty-printer reconstructs the structure
    assert!(output.contains("fn add(a: i64, b: i64) -> i64"));
    assert!(output.contains("return (a + b);"));
    assert!(output.contains("fn main()"));
    assert!(output.contains("let x: i64 = 42;"));
    assert!(output.contains("let mut i: i64 = 0;"));
    assert!(output.contains("while (i < 10)"));
    assert!(output.contains("i = (i + 1);"));
    assert!(output.contains("if (x == 42)"));
    assert!(output.contains("add(x, i)"));
    assert!(output.contains("add(0, 0)"));
}

// ── Implicit return ─────────────────────────────────────

#[test]
fn test_implicit_return_parsed_as_tail_expr() {
    let prog = parse("fn add(a: i64, b: i64) -> i64 { a + b }");
    // The last expression without `;` should be a TailExpr
    if let Stmt::TailExpr { .. } = &prog.functions[0].body.stmts[0] {
        // good
    } else {
        panic!("expected TailExpr, got {:?}", prog.functions[0].body.stmts[0]);
    }
}

#[test]
fn test_implicit_return_pretty_print() {
    let output = parse_and_print("fn add(a: i64, b: i64) -> i64 { a + b }");
    // TailExpr should be printed without semicolon
    assert!(output.contains("(a + b)\n"));
    assert!(!output.contains("(a + b);"));
}

// ── Pipe operator ───────────────────────────────────────

#[test]
fn test_pipe_desugars_to_call() {
    // `x |> f` should desugar to `f(x)`
    let prog = parse("fn main() { x |> f; }");
    if let Stmt::Expr { expr: Expr::Call { name, args, .. }, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "f");
        assert_eq!(args.len(), 1);
    } else {
        panic!("expected Call from pipe");
    }
}

#[test]
fn test_pipe_with_args_desugars() {
    // `x |> f(y)` should desugar to `f(x, y)`
    let prog = parse("fn main() { x |> f(y); }");
    if let Stmt::Expr { expr: Expr::Call { name, args, .. }, .. } = &prog.functions[0].body.stmts[0] {
        assert_eq!(name, "f");
        assert_eq!(args.len(), 2); // x and y
    } else {
        panic!("expected Call with 2 args from pipe");
    }
}

#[test]
fn test_pipe_chain_desugars() {
    // `x |> f |> g` should desugar to `g(f(x))`
    let output = parse_and_print("fn main() { x |> f |> g; }");
    assert!(output.contains("g(f(x))"));
}

// ── Error cases ─────────────────────────────────────────

#[test]
fn test_missing_semicolon_error() {
    let mut lexer = Lexer::new("fn main() { let x = 1 }");
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens);
    let err = parser.parse_program().unwrap_err();
    assert!(err.message.contains("expected"));
}
