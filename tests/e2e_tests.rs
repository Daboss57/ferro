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
