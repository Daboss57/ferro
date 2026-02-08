use std::env;
use std::fs;
use std::path::Path;
use std::process::{self, Command};

fn print_usage() {
    eprintln!("Usage: ferro <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  compile <file.ferro>    Compile a Ferro source file");
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

    // TODO Phase 2: Lex the source into tokens
    // TODO Phase 3: Parse tokens into an AST
    // TODO Phase 4: Semantic analysis (type checking, name resolution)

    // Phase 1: Emit hardcoded hello-world assembly as proof of concept
    let asm = ferro::codegen::emit_hello_world();

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
