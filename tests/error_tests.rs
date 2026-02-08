use ferro::error::{CompileError, Pos, Span};

#[test]
fn test_error_report_with_span() {
    let source = "let x = 42;\nlet y = ???;\n";
    let span = Span::new(
        Pos { offset: 20, line: 2, column: 9 },
        Pos { offset: 23, line: 2, column: 12 },
    );
    let err = CompileError::new("unexpected token '???'", span);
    // Just verify it doesn't panic
    err.report(source, "test.ferro");
    assert_eq!(err.message, "unexpected token '???'");
}

#[test]
fn test_error_without_span() {
    let err = CompileError::no_span("something went wrong");
    err.report("", "test.ferro");
    assert_eq!(err.message, "something went wrong");
}
