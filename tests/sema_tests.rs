use ferro::lexer::Lexer;
use ferro::parser::Parser;
use ferro::sema::checker::Checker;

/// Helper: run semantic analysis on source code, return Ok or error message.
fn check(source: &str) -> Result<(), String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().expect("lexer error");
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program().expect("parser error");
    let mut checker = Checker::new();
    checker.check_program(&program).map_err(|e| e.message)
}

/// Helper: assert that checking succeeds.
fn check_ok(source: &str) {
    if let Err(msg) = check(source) {
        panic!("expected check to pass, got error: {}", msg);
    }
}

/// Helper: assert that checking fails with a message containing `expected`.
fn check_err(source: &str, expected: &str) {
    match check(source) {
        Ok(()) => panic!("expected error containing '{}', but check passed", expected),
        Err(msg) => {
            assert!(
                msg.contains(expected),
                "error '{}' does not contain '{}'",
                msg,
                expected
            );
        }
    }
}

// ── Valid programs ──────────────────────────────────────

#[test]
fn test_simple_valid_program() {
    check_ok("fn main() { let x: i64 = 42; }");
}

#[test]
fn test_arithmetic_valid() {
    check_ok("fn main() { let x: i64 = 1 + 2 * 3; }");
}

#[test]
fn test_boolean_valid() {
    check_ok("fn main() { let x: bool = true; }");
}

#[test]
fn test_comparison_valid() {
    check_ok("fn main() { let x: i64 = 5; if x < 10 { 1; } }");
}

#[test]
fn test_logical_operators_valid() {
    check_ok("fn main() { let x: bool = true && false || true; }");
}

#[test]
fn test_function_call_valid() {
    check_ok(
        "fn add(a: i64, b: i64) -> i64 { return a + b; }
         fn main() { let x: i64 = add(1, 2); }",
    );
}

#[test]
fn test_if_else_valid() {
    check_ok(
        "fn main() {
            let x: i64 = 10;
            if x == 10 { let y: i64 = 1; } else { let y: i64 = 2; }
        }",
    );
}

#[test]
fn test_while_valid() {
    check_ok(
        "fn main() {
            let mut i: i64 = 0;
            while i < 10 { i = i + 1; }
        }",
    );
}

#[test]
fn test_void_return_valid() {
    check_ok("fn main() { return; }");
}

#[test]
fn test_i64_return_valid() {
    check_ok("fn foo() -> i64 { return 42; }");
}

#[test]
fn test_type_inference_valid() {
    check_ok("fn main() { let x = 42; let y = x + 1; }");
}

#[test]
fn test_nested_scopes_valid() {
    check_ok(
        "fn main() {
            let x: i64 = 1;
            if true { let y: i64 = x + 1; }
        }",
    );
}

#[test]
fn test_print_valid() {
    check_ok("fn main() { print(42); }");
}

#[test]
fn test_unary_neg_valid() {
    check_ok("fn main() { let x: i64 = -42; }");
}

#[test]
fn test_unary_not_valid() {
    check_ok("fn main() { let x: bool = !true; }");
}

#[test]
fn test_equality_bool_valid() {
    check_ok("fn main() { let x: bool = true == false; }");
}

#[test]
fn test_multiple_functions_call_each_other() {
    check_ok(
        "fn double(x: i64) -> i64 { return x + x; }
         fn quad(x: i64) -> i64 { return double(double(x)); }
         fn main() { let r: i64 = quad(5); }",
    );
}

// ── Undeclared variable ─────────────────────────────────

#[test]
fn test_undeclared_variable() {
    check_err(
        "fn main() { let x: i64 = y; }",
        "undeclared variable 'y'",
    );
}

#[test]
fn test_undeclared_in_expr() {
    check_err(
        "fn main() { let x: i64 = 1 + z; }",
        "undeclared variable 'z'",
    );
}

// ── Type mismatches ─────────────────────────────────────

#[test]
fn test_type_mismatch_let() {
    check_err(
        "fn main() { let x: i64 = true; }",
        "type mismatch",
    );
}

#[test]
fn test_type_mismatch_assignment() {
    check_err(
        "fn main() { let mut x: i64 = 0; x = true; }",
        "type mismatch",
    );
}

#[test]
fn test_add_bool_and_int() {
    check_err(
        "fn main() { let x: i64 = true + 1; }",
        "cannot apply",
    );
}

#[test]
fn test_compare_different_types() {
    check_err(
        "fn main() { let x: bool = 42 == true; }",
        "cannot compare",
    );
}

#[test]
fn test_negate_bool() {
    check_err(
        "fn main() { let x: i64 = -true; }",
        "cannot negate",
    );
}

#[test]
fn test_not_int() {
    check_err(
        "fn main() { let x: bool = !42; }",
        "cannot apply '!'",
    );
}

#[test]
fn test_logical_on_int() {
    check_err(
        "fn main() { let x: bool = 1 && 2; }",
        "cannot apply",
    );
}

// ── If/While condition must be bool ─────────────────────

#[test]
fn test_if_condition_not_bool() {
    check_err(
        "fn main() { if 42 { 1; } }",
        "if condition must be 'bool'",
    );
}

#[test]
fn test_while_condition_not_bool() {
    check_err(
        "fn main() { while 1 { 2; } }",
        "while condition must be 'bool'",
    );
}

// ── Function errors ─────────────────────────────────────

#[test]
fn test_undeclared_function() {
    check_err(
        "fn main() { foo(); }",
        "undeclared function 'foo'",
    );
}

#[test]
fn test_wrong_arg_count() {
    check_err(
        "fn add(a: i64, b: i64) -> i64 { return a + b; }
         fn main() { add(1); }",
        "takes 2 argument(s), got 1",
    );
}

#[test]
fn test_wrong_arg_type() {
    check_err(
        "fn add(a: i64, b: i64) -> i64 { return a + b; }
         fn main() { add(1, true); }",
        "expected 'i64', got 'bool'",
    );
}

// ── Return type errors ──────────────────────────────────

#[test]
fn test_wrong_return_type() {
    check_err(
        "fn foo() -> i64 { return true; }",
        "return type mismatch",
    );
}

#[test]
fn test_void_function_returns_value() {
    check_err(
        "fn main() { return 42; }",
        "return type mismatch",
    );
}

// ── Unknown type ────────────────────────────────────────

#[test]
fn test_unknown_type() {
    check_err(
        "fn main() { let x: float = 1; }",
        "unknown type 'float'",
    );
}

// ── Strings (Phase 7) ──────────────────────────────────

#[test]
fn test_string_variable_valid() {
    check_ok(r#"fn main() { let s: str = "hello"; print(s); }"#);
}

#[test]
fn test_string_param_valid() {
    check_ok(r#"fn greet(name: str) { print(name); } fn main() { greet("world"); }"#);
}

#[test]
fn test_string_return_valid() {
    check_ok(r#"fn greeting() -> str { return "hi"; } fn main() { print(greeting()); }"#);
}

#[test]
fn test_string_implicit_return_valid() {
    check_ok(r#"fn greeting() -> str { "hi" } fn main() { print(greeting()); }"#);
}

#[test]
fn test_print_string_valid() {
    check_ok(r#"fn main() { print("hello"); }"#);
}

#[test]
fn test_print_bool_valid() {
    check_ok("fn main() { print(true); print(false); }");
}

#[test]
fn test_string_type_mismatch() {
    check_err(
        r#"fn main() { let x: i64 = "oops"; }"#,
        "type mismatch",
    );
}

#[test]
fn test_string_arithmetic_error() {
    check_err(
        r#"fn main() { let x: str = "a" + "b"; }"#,
        "cannot apply",
    );
}

#[test]
fn test_len_valid() {
    check_ok(r#"fn main() { let n: i64 = len("hello"); print(n); }"#);
}

#[test]
fn test_len_wrong_type() {
    check_err(
        "fn main() { len(42); }",
        "expected 'str', got 'i64'",
    );
}

#[test]
fn test_len_wrong_arg_count() {
    check_err(
        r#"fn main() { len("a", "b"); }"#,
        "takes 1 argument(s), got 2",
    );
}
