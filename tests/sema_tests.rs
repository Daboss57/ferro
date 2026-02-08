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
        "expects 'str' or array",
    );
}

#[test]
fn test_len_wrong_arg_count() {
    check_err(
        r#"fn main() { len("a", "b"); }"#,
        "len() takes 1 argument, got 2",
    );
}

// ── Arrays & Modulo (Phase 8) ──────────────────────────

#[test]
fn test_array_literal_valid() {
    check_ok("fn main() { let arr: [i64; 3] = [1, 2, 3]; print(arr[0]); }");
}

// ── For, Break, Continue, Match (Phase 10) ────────────

#[test]
fn test_for_valid() {
    check_ok("fn main() { for i in 0..10 { print(i); } }");
}

#[test]
fn test_for_variable_bounds() {
    check_ok("fn main() { let n: i64 = 5; for i in 0..n { print(i); } }");
}

#[test]
fn test_for_range_bool_error() {
    check_err(
        "fn main() { for i in true..5 { print(i); } }",
        "i64",
    );
}

#[test]
fn test_break_in_while() {
    check_ok("fn main() { while true { break; } }");
}

#[test]
fn test_break_outside_loop() {
    check_err(
        "fn main() { break; }",
        "outside of loop",
    );
}

#[test]
fn test_continue_outside_loop() {
    check_err(
        "fn main() { continue; }",
        "outside of loop",
    );
}

#[test]
fn test_break_in_for() {
    check_ok("fn main() { for i in 0..10 { break; } }");
}

#[test]
fn test_continue_in_for() {
    check_ok("fn main() { for i in 0..10 { continue; } }");
}

#[test]
fn test_match_int_valid() {
    check_ok(
        "fn main() {
            let x: i64 = 1;
            match x {
                1 => { print(1); }
                _ => { print(0); }
            }
        }"
    );
}

#[test]
fn test_match_bool_valid() {
    check_ok(
        "fn main() {
            let x: bool = true;
            match x {
                true => { print(1); }
                false => { print(0); }
            }
        }"
    );
}

#[test]
fn test_match_pattern_type_mismatch() {
    check_err(
        "fn main() {
            let x: bool = true;
            match x {
                1 => { print(1); }
                _ => { print(0); }
            }
        }",
        "integer pattern in match",
    );
}

#[test]
fn test_array_index_valid() {
    check_ok("fn main() { let arr = [10, 20]; print(arr[0]); }");
}

#[test]
fn test_array_index_assign_valid() {
    check_ok("fn main() { let arr = [1, 2, 3]; arr[0] = 99; }");
}

#[test]
fn test_array_mixed_types_error() {
    check_err(
        "fn main() { let arr = [1, true]; }",
        "array element 1 has type 'bool', expected 'i64'",
    );
}

#[test]
fn test_array_index_non_array_error() {
    check_err(
        "fn main() { let x: i64 = 5; print(x[0]); }",
        "cannot index into non-array",
    );
}

#[test]
fn test_array_index_not_int_error() {
    check_err(
        "fn main() { let arr = [1, 2]; print(arr[true]); }",
        "array index must be 'i64'",
    );
}

#[test]
fn test_array_index_assign_type_mismatch() {
    check_err(
        "fn main() { let arr = [1, 2]; arr[0] = true; }",
        "type mismatch in index assignment",
    );
}

#[test]
fn test_empty_array_error() {
    check_err(
        "fn main() { let arr = []; }",
        "empty array literals are not allowed",
    );
}

#[test]
fn test_len_array_valid() {
    check_ok("fn main() { let arr = [1, 2, 3]; print(len(arr)); }");
}

#[test]
fn test_modulo_valid() {
    check_ok("fn main() { print(10 % 3); }");
}

#[test]
fn test_modulo_type_error() {
    check_err(
        "fn main() { print(true % false); }",
        "cannot apply",
    );
}

// ── Enums (Phase 11) ──────────────────────────────────

#[test]
fn test_enum_valid() {
    check_ok(
        "enum Color { Red, Green, Blue }
        fn main() { let c: Color = Color::Red; }",
    );
}

#[test]
fn test_enum_match_valid() {
    check_ok(
        "enum Color { Red, Green }
        fn main() {
            let c: Color = Color::Red;
            match c {
                Color::Red => { print(0); }
                Color::Green => { print(1); }
            }
        }",
    );
}

#[test]
fn test_enum_unknown_variant() {
    check_err(
        "enum Color { Red, Green }
        fn main() { let c: Color = Color::Blue; }",
        "unknown variant",
    );
}

#[test]
fn test_enum_unknown_enum() {
    check_err(
        "fn main() { let c: Foo = Foo::Bar; }",
        "unknown",
    );
}

#[test]
fn test_enum_type_mismatch_assign() {
    check_err(
        "enum Color { Red }
        fn main() { let c: Color = 42; }",
        "type mismatch",
    );
}

#[test]
fn test_enum_pattern_wrong_enum() {
    check_err(
        "enum Color { Red }
        enum Size { Small }
        fn main() {
            let c: Color = Color::Red;
            match c {
                Size::Small => { print(0); }
            }
        }",
        "enum pattern",
    );
}

#[test]
fn test_enum_function_param() {
    check_ok(
        "enum Dir { Up, Down }
        fn go(d: Dir) { print(0); }
        fn main() { go(Dir::Up); }",
    );
}

#[test]
fn test_enum_return_type() {
    check_ok(
        "enum Answer { Yes, No }
        fn ask() -> Answer { Answer::Yes }
        fn main() { let a: Answer = ask(); }",
    );
}

// ── Struct Sema Tests ──────────────────────────────────────

#[test]
fn test_struct_valid() {
    check_ok(
        "struct Point { x: i64, y: i64 }
        fn main() {
            let p: Point = Point { x: 10, y: 20 };
            print(p.x);
        }",
    );
}

#[test]
fn test_struct_unknown_struct() {
    check_err(
        "fn main() { let p: Foo = Foo { x: 1 }; }",
        "unknown struct",
    );
}

#[test]
fn test_struct_unknown_field() {
    check_err(
        "struct Point { x: i64, y: i64 }
        fn main() { let p: Point = Point { x: 1, y: 2 }; print(p.z); }",
        "has no field 'z'",
    );
}

#[test]
fn test_struct_missing_field() {
    check_err(
        "struct Point { x: i64, y: i64 }
        fn main() { let p: Point = Point { x: 1 }; }",
        "missing field 'y'",
    );
}

#[test]
fn test_struct_field_type_mismatch() {
    check_err(
        "struct Point { x: i64, y: i64 }
        fn main() { let p: Point = Point { x: 1, y: true }; }",
        "expected type 'i64', got 'bool'",
    );
}

#[test]
fn test_struct_field_access_non_struct() {
    check_err(
        "fn main() { let x: i64 = 5; print(x.foo); }",
        "field access on non-struct",
    );
}

#[test]
fn test_struct_field_assign_valid() {
    check_ok(
        "struct Point { x: i64, y: i64 }
        fn main() {
            let p: Point = Point { x: 1, y: 2 };
            p.x = 42;
        }",
    );
}

#[test]
fn test_struct_field_assign_type_mismatch() {
    check_err(
        "struct Point { x: i64, y: i64 }
        fn main() {
            let p: Point = Point { x: 1, y: 2 };
            p.x = true;
        }",
        "expected type 'i64', got 'bool'",
    );
}

#[test]
fn test_struct_type_annotation() {
    check_ok(
        "struct Point { x: i64, y: i64 }
        fn main() {
            let p: Point = Point { x: 10, y: 20 };
            let sum: i64 = p.x + p.y;
            print(sum);
        }",
    );
}

// ── Defer Sema Tests ───────────────────────────────────────

#[test]
fn test_defer_valid() {
    check_ok(
        "fn cleanup() { print(1); }
        fn main() { defer cleanup(); }",
    );
}

#[test]
fn test_defer_undeclared_function() {
    check_err(
        "fn main() { defer unknown(); }",
        "undeclared function",
    );
}

#[test]
fn test_defer_print_valid() {
    check_ok(
        "fn main() { defer print(42); }",
    );
}

// ── Try/Fail Sema Tests ────────────────────────────────────

#[test]
fn test_tryfail_valid() {
    check_ok(
        "fn risky() -> i64 ! str { 42 }
        fn caller() -> i64 ! str { let x: i64 = try risky(); x }
        fn main() { caller(); }",
    );
}

#[test]
fn test_fail_in_non_failable() {
    check_err(
        "fn main() { fail \"error\"; }",
        "only be used in failable",
    );
}

#[test]
fn test_try_in_non_failable() {
    check_err(
        "fn risky() -> i64 ! str { 42 }
        fn main() { let x: i64 = try risky(); }",
        "only be used in failable",
    );
}

#[test]
fn test_try_on_non_failable_function() {
    check_err(
        "fn safe() -> i64 { 42 }
        fn caller() -> i64 ! str { let x: i64 = try safe(); x }
        fn main() { caller(); }",
        "non-failable function",
    );
}

#[test]
fn test_fail_requires_string() {
    check_err(
        "fn risky() -> i64 ! str { fail 42; }
        fn main() { risky(); }",
        "expects a string",
    );
}

#[test]
fn test_failable_return_type() {
    check_ok(
        "fn divide(a: i64, b: i64) -> i64 ! str {
            if b == 0 { fail \"div by zero\"; }
            a / b
        }
        fn main() { divide(10, 2); }",
    );
}
