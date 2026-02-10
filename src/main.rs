use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::process::{self, Command};

fn print_usage() {
    eprintln!("Usage: ferro <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  compile <file.ferro>    Compile a Ferro source file");
    eprintln!("  check <file.ferro>      Check for errors (JSON output)");
    eprintln!("  lsp                     Start Language Server Protocol mode");
    eprintln!("  help                    Show this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "compile" => {
            if args.len() < 3 {
                eprintln!("error: missing source file");
                eprintln!("Usage: ferro compile <file.ferro>");
                process::exit(1);
            }
            let filename = &args[2];
            compile(filename);
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("error: missing source file");
                process::exit(1);
            }
            let filename = &args[2];
            check(filename);
        }
        "lsp" => {
            run_lsp();
        }
        "help" | "--help" | "-h" => {
            print_usage();
        }
        other => {
            eprintln!("error: unknown command '{}'", other);
            print_usage();
            process::exit(1);
        }
    }
}

fn compile(filename: &str) {
    // Read source file
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", filename, e);
            process::exit(1);
        }
    };

    println!("Compiling {} ({} bytes)...", filename, source.len());

    // Phase 2: Lex the source into tokens
    let mut lexer = ferro::lexer::Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            e.report(&source, filename);
            process::exit(1);
        }
    };
    println!("  Lexed {} tokens", tokens.len());

    // Phase 3: Parse tokens into an AST
    let mut parser = ferro::parser::Parser::new(tokens);
    let initial_program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            e.report(&source, filename);
            process::exit(1);
        }
    };

    // Phase 11: Resolve imports and merge modules
    let (program, imported_privates) = if initial_program.imports.is_empty() {
        // No imports — use the program as-is
        println!("  Parsed {} function(s), {} enum(s), {} struct(s), {} comptime(s)", 
            initial_program.functions.len(), initial_program.enums.len(), 
            initial_program.structs.len(), initial_program.comptimes.len());
        (initial_program, Vec::new())
    } else {
        // Has imports — use module resolver
        let file_path = Path::new(filename);
        match ferro::modules::resolve_imports(&source, file_path) {
            Ok((merged, imported_privates)) => {
                println!("  Resolved {} import(s)", initial_program.imports.len());
                println!("  Merged: {} function(s), {} enum(s), {} struct(s), {} comptime(s)", 
                    merged.functions.len(), merged.enums.len(), 
                    merged.structs.len(), merged.comptimes.len());
                (merged, imported_privates)
            }
            Err(e) => {
                e.report(&source, filename);
                process::exit(1);
            }
        }
    };

    println!();
    println!("--- AST ---");
    println!("{}", ferro::ast::pretty::pretty_print(&program));
    println!("-----------");

    // Phase 4: Semantic analysis (type checking, name resolution)
    let mut checker = ferro::sema::checker::Checker::new();
    checker.set_imported_privates(&imported_privates);
    if let Err(e) = checker.check_program(&program) {
        e.report(&source, filename);
        process::exit(1);
    }
    println!("  Type check passed!");

    // Phase 5: Generate x86-64 assembly
    let codegen = ferro::codegen::Codegen::new();
    let asm = codegen.generate(&program);

    // Derive output filenames from input
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let asm_path = format!("{}.s", stem);
    let exe_path = format!("{}.exe", stem);

    // Write assembly to file
    fs::write(&asm_path, &asm).unwrap_or_else(|e| {
        eprintln!("error: could not write '{}': {}", asm_path, e);
        process::exit(1);
    });
    println!("  Wrote assembly to {}", asm_path);

    // Assemble and link with GCC
    let status = Command::new("gcc")
        .args([&asm_path, "-o", &exe_path, "-no-pie"])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("  Linked executable: {}", exe_path);
            // Clean up .s file
            let _ = fs::remove_file(&asm_path);
        }
        Ok(s) => {
            eprintln!("error: gcc exited with {}", s);
            process::exit(1);
        }
        Err(e) => {
            eprintln!("error: could not run gcc: {}", e);
            eprintln!("hint: make sure MinGW/GCC is installed and on your PATH");
            process::exit(1);
        }
    }

    println!("Done! Run .\\{} to execute.", exe_path);
}

/// Check a source file for errors and output JSON diagnostics.
fn check(filename: &str) {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            println!(r#"[{{"message":"could not read file: {}","line":1,"column":1,"end_line":1,"end_column":1}}]"#, e);
            return;
        }
    };
    let diagnostics = check_source(&source);
    println!("{}", diagnostics);
}

/// Run all compiler phases on source code and return JSON array of diagnostics.
fn check_source(source: &str) -> String {
    let mut errors: Vec<String> = Vec::new();

    // Lex
    let mut lexer = ferro::lexer::Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            errors.push(error_to_json(&e));
            return format!("[{}]", errors.join(","));
        }
    };

    // Parse
    let mut parser = ferro::parser::Parser::new(tokens);
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            errors.push(error_to_json(&e));
            return format!("[{}]", errors.join(","));
        }
    };

    // Sema
    let mut checker = ferro::sema::checker::Checker::new();
    if let Err(e) = checker.check_program(&program) {
        errors.push(error_to_json(&e));
    }

    format!("[{}]", errors.join(","))
}

fn error_to_json(e: &ferro::error::CompileError) -> String {
    let (line, col, end_line, end_col) = match &e.span {
        Some(s) => (s.start.line, s.start.column, s.end.line, s.end.column.max(s.start.column + 1)),
        None => (1, 1, 1, 2),
    };
    // Escape the message for JSON
    let msg = e.message.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"{{"message":"{}","line":{},"column":{},"end_line":{},"end_column":{}}}"#,
        msg, line, col, end_line, end_col
    )
}

/// Simple LSP server — reads JSON-RPC over stdin, writes to stdout.
fn run_lsp() {
    // Unbuffered stderr for logging
    eprintln!("Ferro LSP server starting...");

    let stdin = io::stdin();
    let stdout = io::stdout();

    // Track open documents: uri → content
    let mut documents: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    loop {
        // Read LSP header: "Content-Length: N\r\n\r\n"
        let content_length = match read_lsp_header(&stdin) {
            Some(len) => len,
            None => break, // EOF
        };

        // Read body
        let mut body = vec![0u8; content_length];
        stdin.lock().read_exact(&mut body).unwrap_or_else(|_| process::exit(0));
        let body_str = String::from_utf8_lossy(&body);

        // Minimal JSON parsing (no serde dependency)
        let method = json_get_string(&body_str, "method");
        let id = json_get_number(&body_str, "id");

        match method.as_deref() {
            Some("initialize") => {
                let response = format!(
                    r#"{{"jsonrpc":"2.0","id":{},"result":{{"capabilities":{{"textDocumentSync":1,"diagnosticProvider":{{"interFileDependencies":false,"workspaceDiagnostics":false}}}},"serverInfo":{{"name":"ferro-lsp","version":"0.1.0"}}}}}}"#,
                    id.unwrap_or(0)
                );
                send_lsp_message(&stdout, &response);
            }
            Some("initialized") => {
                eprintln!("Ferro LSP initialized");
            }
            Some("textDocument/didOpen") => {
                if let (Some(uri), Some(text)) = (
                    json_get_nested_string(&body_str, "textDocument", "uri"),
                    json_get_nested_string(&body_str, "textDocument", "text"),
                ) {
                    documents.insert(uri.clone(), text.clone());
                    let diag_json = check_source(&text);
                    let notification = format!(
                        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":{}}}}}"#,
                        uri, diagnostics_to_lsp(&diag_json)
                    );
                    send_lsp_message(&stdout, &notification);
                }
            }
            Some("textDocument/didChange") => {
                if let Some(uri) = json_get_nested_string(&body_str, "textDocument", "uri") {
                    // Full sync mode — contentChanges[0].text has the full document
                    if let Some(text) = json_get_content_change_text(&body_str) {
                        documents.insert(uri.clone(), text.clone());
                        let diag_json = check_source(&text);
                        let notification = format!(
                            r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":{}}}}}"#,
                            uri, diagnostics_to_lsp(&diag_json)
                        );
                        send_lsp_message(&stdout, &notification);
                    }
                }
            }
            Some("textDocument/didClose") => {
                if let Some(uri) = json_get_nested_string(&body_str, "textDocument", "uri") {
                    documents.remove(&uri);
                    // Clear diagnostics
                    let notification = format!(
                        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":[]}}}}"#,
                        uri
                    );
                    send_lsp_message(&stdout, &notification);
                }
            }
            Some("shutdown") => {
                let response = format!(r#"{{"jsonrpc":"2.0","id":{},"result":null}}"#, id.unwrap_or(0));
                send_lsp_message(&stdout, &response);
            }
            Some("exit") => {
                process::exit(0);
            }
            _ => {
                // Unknown method — respond with null for requests (those with id)
                if let Some(req_id) = id {
                    let response = format!(r#"{{"jsonrpc":"2.0","id":{},"result":null}}"#, req_id);
                    send_lsp_message(&stdout, &response);
                }
            }
        }
    }
}

fn read_lsp_header(stdin: &io::Stdin) -> Option<usize> {
    let mut length = 0usize;
    let reader = stdin.lock();
    for line in reader.lines() {
        let line = line.ok()?;
        if line.is_empty() {
            break; // end of headers
        }
        if let Some(val) = line.strip_prefix("Content-Length: ") {
            length = val.trim().parse().ok()?;
        }
    }
    if length == 0 { None } else { Some(length) }
}

fn send_lsp_message(stdout: &io::Stdout, msg: &str) {
    let mut out = stdout.lock();
    write!(out, "Content-Length: {}\r\n\r\n{}", msg.len(), msg).unwrap();
    out.flush().unwrap();
}

/// Convert our check_source JSON array to LSP diagnostic array.
fn diagnostics_to_lsp(check_json: &str) -> String {
    // check_json is like: [{"message":"...","line":1,"column":1,"end_line":1,"end_column":5}]
    // LSP wants: [{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":4}},"severity":1,"message":"..."}]
    if check_json == "[]" {
        return "[]".to_string();
    }

    let mut result = Vec::new();
    // Parse each diagnostic object manually
    let inner = &check_json[1..check_json.len()-1]; // strip [ ]
    // Split by },{ but keep it simple — split on "},{" 
    let entries: Vec<&str> = if inner.contains("},{") {
        inner.split("},{").collect()
    } else {
        vec![inner]
    };

    for (i, entry) in entries.iter().enumerate() {
        let mut e = entry.to_string();
        if i > 0 { e = format!("{{{}", e); }
        if i < entries.len() - 1 { e = format!("{}}}", e); }

        let msg = json_get_string(&e, "message").unwrap_or_default();
        let line = json_get_number(&e, "line").unwrap_or(1) as usize;
        let col = json_get_number(&e, "column").unwrap_or(1) as usize;
        let end_line = json_get_number(&e, "end_line").unwrap_or(line as i64) as usize;
        let end_col = json_get_number(&e, "end_column").unwrap_or(col as i64 + 1) as usize;

        // LSP uses 0-based line/character
        let lsp_diag = format!(
            r#"{{"range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"severity":1,"source":"ferro","message":"{}"}}"#,
            line.saturating_sub(1), col.saturating_sub(1),
            end_line.saturating_sub(1), end_col.saturating_sub(1),
            msg.replace('\\', "\\\\").replace('"', "\\\"")
        );
        result.push(lsp_diag);
    }

    format!("[{}]", result.join(","))
}

// ── Minimal JSON helpers (no serde) ─────────────────────

fn json_get_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}""#, key);
    let start = json.find(&pattern)?;
    let after_key = start + pattern.len();
    // Skip `:` and whitespace, find the opening `"`
    let rest = &json[after_key..];
    let colon = rest.find(':')?;
    let after_colon = &rest[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let str_start = 1; // skip opening "
    let str_content = &after_colon[str_start..];
    // Find closing " (handle escaped quotes)
    let mut end = 0;
    let chars: Vec<char> = str_content.chars().collect();
    while end < chars.len() {
        if chars[end] == '\\' {
            end += 2; // skip escape
        } else if chars[end] == '"' {
            break;
        } else {
            end += 1;
        }
    }
    let value: String = chars[..end].iter().collect();
    // Unescape
    Some(value.replace("\\n", "\n").replace("\\t", "\t").replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn json_get_number(json: &str, key: &str) -> Option<i64> {
    let pattern = format!(r#""{}""#, key);
    let start = json.find(&pattern)?;
    let after_key = start + pattern.len();
    let rest = &json[after_key..];
    let colon = rest.find(':')?;
    let after_colon = rest[colon + 1..].trim_start();
    // Read digits (and optional minus)
    let mut num_str = String::new();
    for ch in after_colon.chars() {
        if ch == '-' || ch.is_ascii_digit() {
            num_str.push(ch);
        } else {
            break;
        }
    }
    num_str.parse().ok()
}

fn json_get_nested_string(json: &str, outer_key: &str, inner_key: &str) -> Option<String> {
    // Find "outer_key" : { ... "inner_key": "value" ... }
    let pattern = format!(r#""{}""#, outer_key);
    let start = json.find(&pattern)?;
    let rest = &json[start..];
    // Find the opening { of the nested object
    let brace = rest.find('{')?;
    let nested = &rest[brace..];
    json_get_string(nested, inner_key)
}

fn json_get_content_change_text(json: &str) -> Option<String> {
    // contentChanges is an array: "contentChanges":[{"text":"..."}]
    let start = json.find("\"contentChanges\"")?;
    let rest = &json[start..];
    let bracket = rest.find('[')?;
    let inner = &rest[bracket..];
    json_get_string(inner, "text")
}
