use std::process::Command;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Helper: compile a Ferro source string, run the resulting exe, return stdout.
fn compile_and_run(source: &str) -> String {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let test_dir = std::env::temp_dir().join("ferro_tests");
    fs::create_dir_all(&test_dir).unwrap();

    let ferro_path = test_dir.join(format!("test_{}.ferro", id));
    let asm_path = test_dir.join(format!("test_{}.s", id));
    let exe_path = test_dir.join(format!("test_{}.exe", id));

    fs::write(&ferro_path, source).unwrap();

    // Lex → Parse → Check → Codegen (in-process)
    let mut lexer = ferro::lexer::Lexer::new(source);
    let tokens = lexer.tokenize().expect("lexer error");
    let mut parser = ferro::parser::Parser::new(tokens);
    let program = parser.parse_program().expect("parser error");
    let mut checker = ferro::sema::checker::Checker::new();
    checker.check_program(&program).expect("type check error");
    let codegen = ferro::codegen::Codegen::new();
    let asm = codegen.generate(&program);

    fs::write(&asm_path, &asm).unwrap();

    // Assemble and link
    let status = Command::new("gcc")
        .args([
            asm_path.to_str().unwrap(),
            "-o",
            exe_path.to_str().unwrap(),
            "-no-pie",
        ])
        .status()
        .expect("failed to run gcc");
    assert!(status.success(), "gcc failed");

    // Run the executable and capture output
    let output = Command::new(exe_path.to_str().unwrap())
        .output()
        .expect("failed to run compiled program");

    // Cleanup
    let _ = fs::remove_file(&ferro_path);
    let _ = fs::remove_file(&asm_path);
    let _ = fs::remove_file(&exe_path);

    String::from_utf8(output.stdout).unwrap().replace("\r\n", "\n").trim().to_string()
}

// ── Arithmetic ──────────────────────────────────────────

#[test]
fn test_e2e_basic_arithmetic() {
    assert_eq!(compile_and_run("fn main() { print(1 + 2); }"), "3");
}

#[test]
fn test_e2e_precedence() {
    assert_eq!(compile_and_run("fn main() { print(1 + 2 * 3); }"), "7");
}

#[test]
fn test_e2e_subtraction() {
    assert_eq!(compile_and_run("fn main() { print(10 - 3); }"), "7");
}

#[test]
fn test_e2e_division() {
    assert_eq!(compile_and_run("fn main() { print(42 / 6); }"), "7");
}

#[test]
fn test_e2e_negative() {
    assert_eq!(compile_and_run("fn main() { print(-42); }"), "-42");
}

#[test]
fn test_e2e_parens() {
    assert_eq!(compile_and_run("fn main() { print((1 + 2) * 3); }"), "9");
}

// ── Variables ───────────────────────────────────────────

#[test]
fn test_e2e_variable() {
    assert_eq!(
        compile_and_run("fn main() { let x: i64 = 42; print(x); }"),
        "42"
    );
}

#[test]
fn test_e2e_variable_assignment() {
    assert_eq!(
        compile_and_run("fn main() { let mut x: i64 = 1; x = 99; print(x); }"),
        "99"
    );
}

// ── Functions ───────────────────────────────────────────

#[test]
fn test_e2e_function_call() {
    assert_eq!(
        compile_and_run(
            "fn add(a: i64, b: i64) -> i64 { return a + b; }
             fn main() { print(add(10, 32)); }"
        ),
        "42"
    );
}

#[test]
fn test_e2e_nested_calls() {
    assert_eq!(
        compile_and_run(
            "fn double(x: i64) -> i64 { return x + x; }
             fn main() { print(double(double(5))); }"
        ),
        "20"
    );
}

// ── If/Else ─────────────────────────────────────────────

#[test]
fn test_e2e_if_true() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let x: i64 = 42;
                if x == 42 { print(1); } else { print(0); }
            }"
        ),
        "1"
    );
}

#[test]
fn test_e2e_if_false() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let x: i64 = 99;
                if x == 42 { print(1); } else { print(0); }
            }"
        ),
        "0"
    );
}

// ── While loops ─────────────────────────────────────────

#[test]
fn test_e2e_while_loop() {
    // Sum 0..9 = 45
    assert_eq!(
        compile_and_run(
            "fn main() {
                let mut sum: i64 = 0;
                let mut i: i64 = 0;
                while i < 10 {
                    sum = sum + i;
                    i = i + 1;
                }
                print(sum);
            }"
        ),
        "45"
    );
}

#[test]
fn test_e2e_while_not_entered() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let mut x: i64 = 0;
                while false { x = 99; }
                print(x);
            }"
        ),
        "0"
    );
}

// ── Comparisons ─────────────────────────────────────────

#[test]
fn test_e2e_less_than() {
    assert_eq!(
        compile_and_run("fn main() { if 3 < 5 { print(1); } else { print(0); } }"),
        "1"
    );
}

#[test]
fn test_e2e_greater_than() {
    assert_eq!(
        compile_and_run("fn main() { if 5 > 3 { print(1); } else { print(0); } }"),
        "1"
    );
}

// ── Full programs ───────────────────────────────────────

#[test]
fn test_e2e_fibonacci_iterative() {
    // Compute fib(10) = 55
    assert_eq!(
        compile_and_run(
            "fn main() {
                let mut a: i64 = 0;
                let mut b: i64 = 1;
                let mut i: i64 = 0;
                while i < 10 {
                    let temp: i64 = b;
                    b = a + b;
                    a = temp;
                    i = i + 1;
                }
                print(a);
            }"
        ),
        "55"
    );
}

#[test]
fn test_e2e_multiple_prints() {
    assert_eq!(
        compile_and_run("fn main() { print(1); print(2); print(3); }"),
        "1\n2\n3"
    );
}

// ── Implicit return ─────────────────────────────────────

#[test]
fn test_e2e_implicit_return() {
    assert_eq!(
        compile_and_run(
            "fn double(x: i64) -> i64 { x + x }
             fn main() { print(double(21)); }"
        ),
        "42"
    );
}

#[test]
fn test_e2e_implicit_return_with_other_stmts() {
    assert_eq!(
        compile_and_run(
            "fn compute(x: i64) -> i64 {
                let y: i64 = x * 2;
                y + 1
             }
             fn main() { print(compute(20)); }"
        ),
        "41"
    );
}

// ── Pipe operator |> ────────────────────────────────────

#[test]
fn test_e2e_pipe_simple() {
    // 5 |> double → double(5) = 10
    assert_eq!(
        compile_and_run(
            "fn double(x: i64) -> i64 { x + x }
             fn main() { 5 |> double |> print; }"
        ),
        "10"
    );
}

#[test]
fn test_e2e_pipe_with_args() {
    // 3 |> add(4) → add(3, 4) = 7
    assert_eq!(
        compile_and_run(
            "fn add(a: i64, b: i64) -> i64 { a + b }
             fn main() { 3 |> add(4) |> print; }"
        ),
        "7"
    );
}

#[test]
fn test_e2e_pipe_chain() {
    // 5 |> double |> add(1) |> double → double(add(double(5), 1)) = double(11) = 22
    assert_eq!(
        compile_and_run(
            "fn double(x: i64) -> i64 { x + x }
             fn add(a: i64, b: i64) -> i64 { a + b }
             fn main() { 5 |> double |> add(1) |> double |> print; }"
        ),
        "22"
    );
}

#[test]
fn test_e2e_implicit_return_and_pipe() {
    // Combine both features
    assert_eq!(
        compile_and_run(
            "fn triple(x: i64) -> i64 { x * 3 }
             fn main() { 7 |> triple |> print; }"
        ),
        "21"
    );
}

// ── Strings & I/O (Phase 7) ────────────────────────────

#[test]
fn test_e2e_print_string_literal() {
    assert_eq!(
        compile_and_run(r#"fn main() { print("Hello, World!"); }"#),
        "Hello, World!"
    );
}

#[test]
fn test_e2e_string_variable() {
    assert_eq!(
        compile_and_run(r#"fn main() { let s: str = "Ferro"; print(s); }"#),
        "Ferro"
    );
}

#[test]
fn test_e2e_string_function_param() {
    assert_eq!(
        compile_and_run(
            r#"fn greet(name: str) { print(name); }
               fn main() { greet("world"); }"#
        ),
        "world"
    );
}

#[test]
fn test_e2e_string_return() {
    assert_eq!(
        compile_and_run(
            r#"fn greeting() -> str { "hello" }
               fn main() { print(greeting()); }"#
        ),
        "hello"
    );
}

#[test]
fn test_e2e_string_pipe() {
    assert_eq!(
        compile_and_run(
            r#"fn main() { "piped" |> print; }"#
        ),
        "piped"
    );
}

#[test]
fn test_e2e_len_builtin() {
    assert_eq!(
        compile_and_run(r#"fn main() { print(len("hello")); }"#),
        "5"
    );
}

#[test]
fn test_e2e_len_empty_string() {
    assert_eq!(
        compile_and_run(r#"fn main() { print(len("")); }"#),
        "0"
    );
}

#[test]
fn test_e2e_len_variable() {
    assert_eq!(
        compile_and_run(r#"fn main() { let s: str = "Ferro!"; print(len(s)); }"#),
        "6"
    );
}

#[test]
fn test_e2e_len_in_arithmetic() {
    assert_eq!(
        compile_and_run(r#"fn main() { print(len("abc") + len("de")); }"#),
        "5"
    );
}

#[test]
fn test_e2e_print_bool_true() {
    assert_eq!(
        compile_and_run("fn main() { print(true); }"),
        "true"
    );
}

#[test]
fn test_e2e_print_bool_false() {
    assert_eq!(
        compile_and_run("fn main() { print(false); }"),
        "false"
    );
}

#[test]
fn test_e2e_print_bool_expression() {
    assert_eq!(
        compile_and_run("fn main() { print(3 < 5); }"),
        "true"
    );
}

#[test]
fn test_e2e_print_bool_variable() {
    assert_eq!(
        compile_and_run("fn main() { let flag: bool = true; print(flag); }"),
        "true"
    );
}

#[test]
fn test_e2e_mixed_prints() {
    assert_eq!(
        compile_and_run(
            r#"fn main() {
                print(42);
                print("hello");
                print(true);
            }"#
        ),
        "42\nhello\ntrue"
    );
}

// ── Arrays & Modulo (Phase 8) ──────────────────────────

#[test]
fn test_e2e_modulo() {
    assert_eq!(compile_and_run("fn main() { print(17 % 5); }"), "2");
}

#[test]
fn test_e2e_modulo_even_odd() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                if 4 % 2 == 0 { print(1); } else { print(0); }
            }"
        ),
        "1"
    );
}

#[test]
fn test_e2e_array_literal_and_index() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let arr: [i64; 3] = [10, 20, 30];
                print(arr[0]);
                print(arr[1]);
                print(arr[2]);
            }"
        ),
        "10\n20\n30"
    );
}

#[test]
fn test_e2e_array_index_assign() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let arr: [i64; 3] = [1, 2, 3];
                arr[1] = 99;
                print(arr[1]);
            }"
        ),
        "99"
    );
}

#[test]
fn test_e2e_array_computed_index() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let arr: [i64; 3] = [10, 20, 30];
                let i: i64 = 2;
                print(arr[i]);
            }"
        ),
        "30"
    );
}

#[test]
fn test_e2e_array_len() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let arr: [i64; 4] = [5, 10, 15, 20];
                print(len(arr));
            }"
        ),
        "4"
    );
}

#[test]
fn test_e2e_array_loop_sum() {
    // Sum [10, 20, 30] = 60
    assert_eq!(
        compile_and_run(
            "fn main() {
                let arr: [i64; 3] = [10, 20, 30];
                let mut sum: i64 = 0;
                let mut i: i64 = 0;
                while i < len(arr) {
                    sum = sum + arr[i];
                    i = i + 1;
                }
                print(sum);
            }"
        ),
        "60"
    );
}

#[test]
fn test_e2e_array_with_expressions() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let x: i64 = 5;
                let arr: [i64; 3] = [x, x * 2, x * 3];
                print(arr[0]);
                print(arr[1]);
                print(arr[2]);
            }"
        ),
        "5\n10\n15"
    );
}

#[test]
fn test_e2e_array_type_inferred() {
    // Array without explicit type annotation
    assert_eq!(
        compile_and_run(
            "fn main() {
                let arr = [100, 200, 300];
                print(arr[1]);
            }"
        ),
        "200"
    );
}

// ── For Loops, Break, Continue, Match (Phase 10) ──────

#[test]
fn test_e2e_for_basic() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                for i in 0..5 {
                    print(i);
                }
            }"
        ),
        "0\n1\n2\n3\n4"
    );
}

#[test]
fn test_e2e_for_sum() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let mut sum: i64 = 0;
                for i in 1..6 {
                    sum = sum + i;
                }
                print(sum);
            }"
        ),
        "15"
    );
}

#[test]
fn test_e2e_for_with_variable_bound() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let n: i64 = 4;
                for i in 0..n {
                    print(i);
                }
            }"
        ),
        "0\n1\n2\n3"
    );
}

#[test]
fn test_e2e_for_nested() {
    // Prints i*10 + j for i=0..2, j=0..3
    assert_eq!(
        compile_and_run(
            "fn main() {
                for i in 0..2 {
                    for j in 0..3 {
                        print(i * 10 + j);
                    }
                }
            }"
        ),
        "0\n1\n2\n10\n11\n12"
    );
}

#[test]
fn test_e2e_for_empty_range() {
    // 5..3 should execute 0 times
    assert_eq!(
        compile_and_run(
            "fn main() {
                for i in 5..3 {
                    print(i);
                }
                print(0);
            }"
        ),
        "0"
    );
}

#[test]
fn test_e2e_break_simple() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let mut i: i64 = 0;
                while i < 10 {
                    if i == 3 { break; }
                    print(i);
                    i = i + 1;
                }
            }"
        ),
        "0\n1\n2"
    );
}

#[test]
fn test_e2e_break_in_for() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                for i in 0..100 {
                    if i == 4 { break; }
                    print(i);
                }
            }"
        ),
        "0\n1\n2\n3"
    );
}

#[test]
fn test_e2e_continue_simple() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let mut i: i64 = 0;
                while i < 5 {
                    i = i + 1;
                    if i == 3 { continue; }
                    print(i);
                }
            }"
        ),
        "1\n2\n4\n5"
    );
}

#[test]
fn test_e2e_continue_in_for() {
    // Skip even numbers
    assert_eq!(
        compile_and_run(
            "fn main() {
                for i in 0..6 {
                    if i % 2 == 0 { continue; }
                    print(i);
                }
            }"
        ),
        "1\n3\n5"
    );
}

#[test]
fn test_e2e_match_int() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let x: i64 = 2;
                match x {
                    1 => { print(10); }
                    2 => { print(20); }
                    3 => { print(30); }
                    _ => { print(0); }
                }
            }"
        ),
        "20"
    );
}

#[test]
fn test_e2e_match_wildcard() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let x: i64 = 99;
                match x {
                    1 => { print(10); }
                    _ => { print(42); }
                }
            }"
        ),
        "42"
    );
}

#[test]
fn test_e2e_match_bool() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                let flag: bool = true;
                match flag {
                    true => { print(1); }
                    false => { print(0); }
                }
            }"
        ),
        "1"
    );
}

#[test]
fn test_e2e_match_first_arm() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                match 1 {
                    1 => { print(100); }
                    2 => { print(200); }
                    _ => { print(0); }
                }
            }"
        ),
        "100"
    );
}

#[test]
fn test_e2e_match_in_loop() {
    assert_eq!(
        compile_and_run(
            "fn main() {
                for i in 0..4 {
                    match i {
                        0 => { print(10); }
                        1 => { print(20); }
                        _ => { print(99); }
                    }
                }
            }"
        ),
        "10\n20\n99\n99"
    );
}

// ── Enums (Phase 11) ──────────────────────────────────

#[test]
fn test_e2e_enum_basic() {
    assert_eq!(
        compile_and_run(
            "enum Color { Red, Green, Blue }
            fn main() {
                let c: Color = Color::Green;
                match c {
                    Color::Red => { print(0); }
                    Color::Green => { print(1); }
                    Color::Blue => { print(2); }
                }
            }"
        ),
        "1"
    );
}

#[test]
fn test_e2e_enum_function_param() {
    assert_eq!(
        compile_and_run(
            "enum Dir { Up, Down }
            fn describe(d: Dir) {
                match d {
                    Dir::Up => { print(10); }
                    Dir::Down => { print(20); }
                }
            }
            fn main() {
                describe(Dir::Up);
                describe(Dir::Down);
            }"
        ),
        "10\n20"
    );
}

#[test]
fn test_e2e_enum_return_value() {
    assert_eq!(
        compile_and_run(
            "enum Answer { Yes, No }
            fn get_answer() -> Answer { Answer::Yes }
            fn main() {
                let a: Answer = get_answer();
                match a {
                    Answer::Yes => { print(1); }
                    Answer::No => { print(0); }
                }
            }"
        ),
        "1"
    );
}

#[test]
fn test_e2e_enum_wildcard() {
    assert_eq!(
        compile_and_run(
            "enum Color { Red, Green, Blue }
            fn main() {
                let c: Color = Color::Blue;
                match c {
                    Color::Red => { print(0); }
                    _ => { print(99); }
                }
            }"
        ),
        "99"
    );
}

#[test]
fn test_e2e_enum_comparison() {
    assert_eq!(
        compile_and_run(
            "enum Fruit { Apple, Banana, Cherry }
            fn main() {
                let f: Fruit = Fruit::Banana;
                if f == Fruit::Banana {
                    print(1);
                } else {
                    print(0);
                }
            }"
        ),
        "1"
    );
}

#[test]
fn test_e2e_enum_in_loop() {
    assert_eq!(
        compile_and_run(
            "enum Light { Red, Yellow, Green }
            fn label(l: Light) {
                match l {
                    Light::Red => { print(1); }
                    Light::Yellow => { print(2); }
                    Light::Green => { print(3); }
                }
            }
            fn main() {
                label(Light::Red);
                label(Light::Yellow);
                label(Light::Green);
            }"
        ),
        "1\n2\n3"
    );
}

#[test]
fn test_e2e_enum_multiple_enums() {
    assert_eq!(
        compile_and_run(
            "enum Color { Red, Blue }
            enum Size { Small, Large }
            fn main() {
                let c: Color = Color::Blue;
                let s: Size = Size::Small;
                match c {
                    Color::Red => { print(10); }
                    Color::Blue => { print(20); }
                }
                match s {
                    Size::Small => { print(100); }
                    Size::Large => { print(200); }
                }
            }"
        ),
        "20\n100"
    );
}
