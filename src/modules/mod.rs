// Module resolution — handles `import "path.ferro";` declarations.
//
// Recursively parses imported files and merges all items into a single Program.
// Detects circular imports and marks which module each item comes from.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::error::CompileError;
use crate::lexer::Lexer;
use crate::parser::Parser;

/// Tracks which items are imported (vs defined in main file) for visibility checking.
#[derive(Debug)]
pub struct ModuleItem {
    pub module_path: PathBuf,
    pub is_private: bool,
}

/// Resolve all imports starting from the main file, producing a merged Program.
///
/// - `main_source`: the source code of the main file (already read)
/// - `main_path`: path to the main file (used to resolve relative imports)
///
/// Returns a merged Program containing all items from all modules,
/// plus a map of which items came from imported modules.
pub fn resolve_imports(
    main_source: &str,
    main_path: &Path,
) -> Result<(Program, Vec<(String, bool)>), CompileError> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let main_canonical = canonicalize_path(main_path)?;
    visited.insert(main_canonical.clone());

    // Parse main file
    let mut lexer = Lexer::new(main_source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let main_program = parser.parse_program()?;

    // Collect all items: start with main program items
    let mut all_functions = Vec::new();
    let mut all_enums = Vec::new();
    let mut all_structs = Vec::new();
    let mut all_comptimes = Vec::new();
    // Track which functions are imported (for priv enforcement): (name, is_private)
    let mut imported_privates: Vec<(String, bool)> = Vec::new();

    // Process imports recursively
    let main_dir = main_canonical.parent().unwrap_or(Path::new("."));
    for import in &main_program.imports {
        resolve_one_import(
            &import.path,
            main_dir,
            &mut visited,
            &mut all_functions,
            &mut all_enums,
            &mut all_structs,
            &mut all_comptimes,
            &mut imported_privates,
            import.span,
        )?;
    }

    // Add main file's items (these are never "imported" — they're the main module)
    for f in main_program.functions {
        all_functions.push(f);
    }
    for e in main_program.enums {
        all_enums.push(e);
    }
    for s in main_program.structs {
        all_structs.push(s);
    }
    for c in main_program.comptimes {
        all_comptimes.push(c);
    }

    Ok((
        Program {
            imports: Vec::new(), // already resolved
            functions: all_functions,
            enums: all_enums,
            structs: all_structs,
            comptimes: all_comptimes,
        },
        imported_privates,
    ))
}

fn resolve_one_import(
    import_path: &str,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    functions: &mut Vec<Function>,
    enums: &mut Vec<EnumDef>,
    structs: &mut Vec<StructDef>,
    comptimes: &mut Vec<ComptimeDef>,
    imported_privates: &mut Vec<(String, bool)>,
    span: crate::error::Span,
) -> Result<(), CompileError> {
    // Resolve relative to the importing file's directory
    let full_path = base_dir.join(import_path);
    let canonical = canonicalize_path(&full_path).map_err(|_| {
        CompileError::new(
            format!("could not find imported file '{}'", import_path),
            span,
        )
    })?;

    // Check for circular imports
    if visited.contains(&canonical) {
        // Already imported — skip (not an error, just don't import twice)
        return Ok(());
    }
    visited.insert(canonical.clone());

    // Read and parse the imported file
    let source = std::fs::read_to_string(&canonical).map_err(|e| {
        CompileError::new(
            format!("could not read '{}': {}", import_path, e),
            span,
        )
    })?;

    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|mut e| {
        e.message = format!("in {}: {}", import_path, e.message);
        e
    })?;
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program().map_err(|mut e| {
        e.message = format!("in {}: {}", import_path, e.message);
        e
    })?;

    // Recursively resolve this file's imports
    let import_dir = canonical.parent().unwrap_or(Path::new("."));
    for sub_import in &program.imports {
        resolve_one_import(
            &sub_import.path,
            import_dir,
            visited,
            functions,
            enums,
            structs,
            comptimes,
            imported_privates,
            sub_import.span,
        )?;
    }

    // Add this module's items, tracking private status
    for f in program.functions {
        imported_privates.push((f.name.clone(), f.is_private));
        functions.push(f);
    }
    for e in program.enums {
        imported_privates.push((e.name.clone(), e.is_private));
        enums.push(e);
    }
    for s in program.structs {
        imported_privates.push((s.name.clone(), s.is_private));
        structs.push(s);
    }
    for c in program.comptimes {
        imported_privates.push((c.name.clone(), c.is_private));
        comptimes.push(c);
    }

    Ok(())
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, CompileError> {
    // Try to canonicalize; if file doesn't exist yet, just normalize
    std::fs::canonicalize(path).map_err(|e| {
        CompileError::no_span(format!("could not resolve path '{}': {}", path.display(), e))
    })
}
