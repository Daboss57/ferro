// Code generation — emits x86-64 assembly (Windows x64 ABI, AT&T syntax for GCC).
//
// Windows x64 calling convention:
//   - First 4 integer args: RCX, RDX, R8, R9
//   - Return value in RAX
//   - Caller must reserve 32 bytes of "shadow space" on the stack
//   - Stack must be 16-byte aligned before CALL
//
// Our approach:
//   - Each function gets a stack frame for local variables
//   - Variables are accessed via RBP offsets: -8(%rbp), -16(%rbp), etc.
//   - Expressions evaluate into RAX (accumulator pattern)
//   - For binary ops: evaluate left → push → evaluate right → pop left → operate

use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::*;

/// Simple type tag so codegen knows how to print / handle each value.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ValType {
    Int,
    Bool,
    Str,
    Void,
}

/// Metadata for a local variable on the stack.
#[derive(Debug, Clone)]
struct LocalVar {
    offset: i64,
    ty: ValType,
    array_len: usize, // 0 = scalar, >0 = fixed-size array
    struct_name: Option<String>, // Some("Point") if this is a struct variable
}

/// Code generator state.
pub struct Codegen {
    /// The output assembly string.
    output: String,
    /// Counter for generating unique labels (for if/else, while, etc.)
    label_counter: usize,
    /// String literals collected during codegen, emitted in .data section.
    string_literals: Vec<String>,
    /// Maps variable name → local variable metadata.
    locals: HashMap<String, LocalVar>,
    /// Next available stack offset for a new local variable.
    stack_offset: i64,
    /// Maps function name → return type (for infer_type on Call exprs).
    func_return_types: HashMap<String, ValType>,
    /// Stack of (continue_label, break_label) for nested loops.
    loop_labels: Vec<(String, String)>,
    /// Maps "EnumName::Variant" → integer discriminant value.
    enum_values: HashMap<String, i64>,
    /// Maps struct name → list of (field_name, field_type) in order.
    struct_fields: HashMap<String, Vec<(String, ValType)>>,
    /// Deferred expressions to run before function return (LIFO order).
    deferred: Vec<Expr>,
    /// Set of function names that are failable (-> T ! str).
    failable_funcs: HashMap<String, bool>,
    /// Whether the current function being generated is failable.
    current_failable: bool,
    /// Comptime constants: name → evaluated integer value.
    comptime_values: HashMap<String, i64>,
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            label_counter: 0,
            string_literals: Vec::new(),
            locals: HashMap::new(),
            stack_offset: 0,
            func_return_types: HashMap::new(),
            loop_labels: Vec::new(),
            enum_values: HashMap::new(),
            struct_fields: HashMap::new(),
            deferred: Vec::new(),
            failable_funcs: HashMap::new(),
            current_failable: false,
            comptime_values: HashMap::new(),
        }
    }

    /// Generate a unique label name.
    fn new_label(&mut self, prefix: &str) -> String {
        self.label_counter += 1;
        format!(".L{}_{}", prefix, self.label_counter)
    }

    /// Allocate a local variable on the stack, return its RBP offset.
    fn alloc_local(&mut self, name: &str, ty: ValType) -> i64 {
        self.stack_offset -= 8;
        let offset = self.stack_offset;
        self.locals.insert(name.to_string(), LocalVar { offset, ty, array_len: 0, struct_name: None });
        offset
    }

    /// Emit a line of assembly.
    fn emit(&mut self, line: &str) {
        writeln!(self.output, "    {}", line).unwrap();
    }

    /// Emit a label.
    fn emit_label(&mut self, label: &str) {
        writeln!(self.output, "{}:", label).unwrap();
    }

    /// Emit all deferred expressions in reverse order (LIFO).
    fn emit_deferred(&mut self) {
        let deferred = self.deferred.clone();
        for expr in deferred.iter().rev() {
            self.gen_expr(expr);
        }
    }

    // ── Type inference (for print dispatch) ────────────────

    /// Determine the runtime type of an expression.
    /// The semantic checker already validated everything — this is just
    /// so codegen knows which printf format / C function to use.
    fn infer_type(&self, expr: &Expr) -> ValType {
        match expr {
            Expr::IntLit { .. } => ValType::Int,
            Expr::BoolLit { .. } => ValType::Bool,
            Expr::StringLit { .. } => ValType::Str,
            Expr::Ident { name, .. } => {
                self.locals.get(name.as_str()).map(|l| l.ty).unwrap_or(ValType::Int)
            }
            Expr::UnaryOp { op, .. } => match op {
                UnaryOp::Neg => ValType::Int,
                UnaryOp::Not => ValType::Bool,
            },
            Expr::BinaryOp { op, left, .. } => match op {
                BinOp::Add => {
                    // String concat returns Str, integer add returns Int
                    if self.infer_type(left) == ValType::Str { ValType::Str } else { ValType::Int }
                }
                BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => ValType::Int,
                _ => ValType::Bool,
            },
            Expr::Call { name, .. } => {
                if name == "print" {
                    ValType::Void
                } else if name == "len" {
                    ValType::Int
                } else {
                    self.func_return_types.get(name.as_str()).copied().unwrap_or(ValType::Int)
                }
            }
            Expr::ArrayLit { .. } => ValType::Int, // array as a whole isn't a single value
            Expr::Index { object, .. } => {
                // Indexing returns the element type
                if let Expr::Ident { name, .. } = object.as_ref() {
                    self.locals.get(name.as_str()).map(|l| l.ty).unwrap_or(ValType::Int)
                } else {
                    ValType::Int
                }
            }
            Expr::EnumVariant { .. } => ValType::Int, // enums stored as i64
            Expr::StructLit { .. } => ValType::Int, // struct as a whole isn't a single value
            Expr::FieldAccess { object, field, .. } => {
                if let Expr::Ident { name, .. } = object.as_ref() {
                    if let Some(var) = self.locals.get(name.as_str()) {
                        if let Some(sname) = &var.struct_name {
                            if let Some(info) = self.struct_fields.get(sname) {
                                if let Some((_, ty)) = info.iter().find(|(n, _)| n == field) {
                                    return *ty;
                                }
                            }
                        }
                    }
                }
                ValType::Int
            }
            Expr::Try { expr, .. } => {
                // try unwraps a failable call — same type as the call's return
                self.infer_type(expr)
            }
            Expr::Cast { target, .. } => {
                Self::valtype_from_name(target)
            }
        }
    }

    /// Convert a type-name string (from the AST) to a ValType.
    fn valtype_from_name(name: &str) -> ValType {
        match name {
            "i64"  => ValType::Int,
            "bool" => ValType::Bool,
            "str"  => ValType::Str,
            _      => ValType::Int,
        }
    }

    /// Evaluate a compile-time constant expression.
    fn eval_comptime_expr(expr: &Expr, known: &HashMap<String, i64>) -> i64 {
        match expr {
            Expr::IntLit { value, .. } => *value,
            Expr::BoolLit { value, .. } => if *value { 1 } else { 0 },
            Expr::Ident { name, .. } => *known.get(name).unwrap_or(&0),
            Expr::UnaryOp { op, operand, .. } => {
                let v = Self::eval_comptime_expr(operand, known);
                match op {
                    UnaryOp::Neg => -v,
                    UnaryOp::Not => if v == 0 { 1 } else { 0 },
                }
            }
            Expr::BinaryOp { op, left, right, .. } => {
                let l = Self::eval_comptime_expr(left, known);
                let r = Self::eval_comptime_expr(right, known);
                match op {
                    BinOp::Add => l + r,
                    BinOp::Sub => l - r,
                    BinOp::Mul => l * r,
                    BinOp::Div => if r != 0 { l / r } else { 0 },
                    BinOp::Mod => if r != 0 { l % r } else { 0 },
                    BinOp::Eq  => if l == r { 1 } else { 0 },
                    BinOp::Neq => if l != r { 1 } else { 0 },
                    BinOp::Lt  => if l < r  { 1 } else { 0 },
                    BinOp::Gt  => if l > r  { 1 } else { 0 },
                    BinOp::Lte => if l <= r { 1 } else { 0 },
                    BinOp::Gte => if l >= r { 1 } else { 0 },
                    BinOp::And => if l != 0 && r != 0 { 1 } else { 0 },
                    BinOp::Or  => if l != 0 || r != 0 { 1 } else { 0 },
                }
            }
            _ => 0,
        }
    }

    // ── Program ─────────────────────────────────────────

    /// Generate assembly for an entire program.
    pub fn generate(mut self, program: &Program) -> String {
        // Register enum variant → discriminant mappings
        for enum_def in &program.enums {
            for (i, variant) in enum_def.variants.iter().enumerate() {
                let key = format!("{}::{}", enum_def.name, variant);
                self.enum_values.insert(key, i as i64);
            }
        }

        // Register struct field info
        for struct_def in &program.structs {
            let fields: Vec<(String, ValType)> = struct_def.fields.iter()
                .map(|f| (f.name.clone(), Self::valtype_from_name(&f.type_name)))
                .collect();
            self.struct_fields.insert(struct_def.name.clone(), fields);
        }

        // Evaluate and register comptime constants
        for ct in &program.comptimes {
            let val = Self::eval_comptime_expr(&ct.value, &self.comptime_values);
            self.comptime_values.insert(ct.name.clone(), val);
        }

        // Collect function return types so infer_type can resolve Call exprs.
        for func in &program.functions {
            let rt = match &func.return_type {
                Some(n) => Self::valtype_from_name(n),
                None => ValType::Void,
            };
            self.func_return_types.insert(func.name.clone(), rt);
            self.failable_funcs.insert(func.name.clone(), func.can_fail);
        }

        // Register built-in function return types
        self.func_return_types.insert("print".to_string(), ValType::Void);
        self.func_return_types.insert("len".to_string(), ValType::Int);
        self.func_return_types.insert("abs".to_string(), ValType::Int);
        self.func_return_types.insert("min".to_string(), ValType::Int);
        self.func_return_types.insert("max".to_string(), ValType::Int);
        self.func_return_types.insert("pow".to_string(), ValType::Int);
        self.func_return_types.insert("rand".to_string(), ValType::Int);
        self.func_return_types.insert("to_str".to_string(), ValType::Str);
        self.func_return_types.insert("parse_int".to_string(), ValType::Int);
        self.func_return_types.insert("char_at".to_string(), ValType::Int);
        self.func_return_types.insert("contains".to_string(), ValType::Bool);
        self.func_return_types.insert("starts_with".to_string(), ValType::Bool);
        self.func_return_types.insert("read_line".to_string(), ValType::Str);
        self.func_return_types.insert("read_file".to_string(), ValType::Str);
        self.func_return_types.insert("write_file".to_string(), ValType::Void);
        self.func_return_types.insert("file_exists".to_string(), ValType::Bool);
        self.func_return_types.insert("eprint".to_string(), ValType::Void);
        self.func_return_types.insert("exit".to_string(), ValType::Void);
        self.func_return_types.insert("time".to_string(), ValType::Int);
        self.func_return_types.insert("sleep".to_string(), ValType::Void);
        self.func_return_types.insert("substr".to_string(), ValType::Str);
        self.func_return_types.insert("trim".to_string(), ValType::Str);
        self.func_return_types.insert("alloc".to_string(), ValType::Int);
        self.func_return_types.insert("free".to_string(), ValType::Void);
        // Register failable builtins
        self.failable_funcs.insert("parse_int".to_string(), true);
        self.failable_funcs.insert("read_file".to_string(), true);
        self.failable_funcs.insert("write_file".to_string(), true);
        // Emit text section
        writeln!(self.output, "    .section .text").unwrap();

        for func in &program.functions {
            self.gen_function(func);
        }

        // Emit built-in helper functions for complex operations
        self.emit_builtin_helpers();

        // Emit data section with string literals
        if !self.string_literals.is_empty() {
            writeln!(self.output, "    .section .data").unwrap();
            for (i, s) in self.string_literals.iter().enumerate() {
                writeln!(self.output, ".Lstr_{}:", i).unwrap();
                // Escape the string for assembly
                writeln!(self.output, "    .ascii \"{}\\0\"", s).unwrap();
            }
        }

        self.output
    }

    // ── Functions ───────────────────────────────────────

    fn gen_function(&mut self, func: &Function) {
        // Reset locals and deferred for each function
        self.locals.clear();
        self.stack_offset = 0;
        self.deferred.clear();
        self.current_failable = func.can_fail;

        // Count how many locals we need (params + local vars in body)
        let local_count = count_locals(func);
        // Stack space: locals * 8, rounded up to 16-byte alignment
        // Plus 32 bytes shadow space for any calls we make
        let frame_size = align16((local_count as i64) * 8 + 32);

        // Function label — priv functions don't get .globl
        if !func.is_private {
            writeln!(self.output, "    .globl {}", func.name).unwrap();
        }
        self.emit_label(&func.name);

        // Prologue: save old base pointer, set up new frame
        self.emit("pushq %rbp");
        self.emit("movq %rsp, %rbp");
        self.emit(&format!("subq ${}, %rsp", frame_size));

        // Store parameters into local variables
        let param_regs = ["%rcx", "%rdx", "%r8", "%r9"];
        for (i, param) in func.params.iter().enumerate() {
            let ty = Self::valtype_from_name(&param.type_name);
            let offset = self.alloc_local(&param.name, ty);
            if i < param_regs.len() {
                self.emit(&format!("movq {}, {}(%rbp)", param_regs[i], offset));
            }
            // Args beyond 4 would be on the stack — not handled yet
        }

        // Generate body
        self.gen_block(&func.body);

        // Emit deferred expressions before epilogue (LIFO)
        self.emit_deferred();

        // If we fall through without a return, return 0 (success)
        self.emit("xorl %eax, %eax");
        if func.can_fail {
            self.emit("xorl %edx, %edx"); // RDX=0 means success
        }

        // Epilogue
        self.emit_label(&format!(".L{}_epilogue", func.name));
        self.emit("movq %rbp, %rsp");
        self.emit("popq %rbp");
        self.emit("ret");
        writeln!(self.output).unwrap();
    }

    /// Emit assembly helper functions for complex built-in operations.
    fn emit_builtin_helpers(&mut self) {
        // __ferro_read_file(path: *const u8) -> (RAX: *const u8, RDX: error flag)
        // Reads entire file into malloc'd buffer. Returns error string if fopen fails.
        let rf_err_idx = self.string_literals.len();
        self.string_literals.push("could not open file".to_string());
        let rf_mode_idx = self.string_literals.len();
        self.string_literals.push("rb".to_string());

        writeln!(self.output, "__ferro_read_file:").unwrap();
        self.emit("pushq %rbp");
        self.emit("movq %rsp, %rbp");
        self.emit("subq $64, %rsp");  // local space
        // Save path
        self.emit("movq %rcx, -8(%rbp)");
        // fopen(path, "rb")
        self.emit(&format!("leaq .Lstr_{}(%rip), %rdx", rf_mode_idx));
        self.emit("call fopen");
        self.emit("testq %rax, %rax");
        self.emit("jnz .Lrf_opened");
        // Error: could not open
        self.emit(&format!("leaq .Lstr_{}(%rip), %rax", rf_err_idx));
        self.emit("movq $1, %rdx");
        self.emit("jmp .Lrf_done");

        self.emit_label(".Lrf_opened");
        self.emit("movq %rax, -16(%rbp)"); // save FILE*
        // fseek(file, 0, SEEK_END=2)
        self.emit("movq %rax, %rcx");
        self.emit("xorl %edx, %edx");
        self.emit("movq $2, %r8");
        self.emit("call fseek");
        // ftell(file) → size
        self.emit("movq -16(%rbp), %rcx");
        self.emit("call ftell");
        self.emit("movq %rax, -24(%rbp)"); // save size
        // fseek(file, 0, SEEK_SET=0)
        self.emit("movq -16(%rbp), %rcx");
        self.emit("xorl %edx, %edx");
        self.emit("xorl %r8d, %r8d");
        self.emit("call fseek");
        // malloc(size + 1)
        self.emit("movq -24(%rbp), %rcx");
        self.emit("incq %rcx");
        self.emit("call malloc");
        self.emit("movq %rax, -32(%rbp)"); // save buffer
        // fread(buf, 1, size, file)
        self.emit("movq %rax, %rcx");
        self.emit("movq $1, %rdx");
        self.emit("movq -24(%rbp), %r8");
        self.emit("movq -16(%rbp), %r9");
        self.emit("call fread");
        // null-terminate
        self.emit("movq -32(%rbp), %rax");
        self.emit("movq -24(%rbp), %rcx");
        self.emit("movb $0, (%rax,%rcx,1)");
        // fclose(file)
        self.emit("movq -16(%rbp), %rcx");
        self.emit("pushq %rax");
        self.emit("call fclose");
        self.emit("popq %rax");
        // Return: RAX = buffer, RDX = 0 (success)
        self.emit("xorl %edx, %edx");

        self.emit_label(".Lrf_done");
        self.emit("movq %rbp, %rsp");
        self.emit("popq %rbp");
        self.emit("ret");
        writeln!(self.output).unwrap();

        // __ferro_write_file(path: RCX, content: RDX) -> (RAX: error msg, RDX: error flag)
        let wf_err_idx = self.string_literals.len();
        self.string_literals.push("could not open file for writing".to_string());
        let wf_mode_idx = self.string_literals.len();
        self.string_literals.push("w".to_string());

        writeln!(self.output, "__ferro_write_file:").unwrap();
        self.emit("pushq %rbp");
        self.emit("movq %rsp, %rbp");
        self.emit("subq $48, %rsp");
        self.emit("movq %rcx, -8(%rbp)");  // save path
        self.emit("movq %rdx, -16(%rbp)"); // save content
        // fopen(path, "w")
        self.emit(&format!("leaq .Lstr_{}(%rip), %rdx", wf_mode_idx));
        self.emit("call fopen");
        self.emit("testq %rax, %rax");
        self.emit("jnz .Lwf_opened");
        // Error
        self.emit(&format!("leaq .Lstr_{}(%rip), %rax", wf_err_idx));
        self.emit("movq $1, %rdx");
        self.emit("jmp .Lwf_done");

        self.emit_label(".Lwf_opened");
        self.emit("movq %rax, -24(%rbp)"); // save FILE*
        // fputs(content, file)
        self.emit("movq -16(%rbp), %rcx"); // content
        self.emit("movq %rax, %rdx");      // file
        self.emit("call fputs");
        // fclose(file)
        self.emit("movq -24(%rbp), %rcx");
        self.emit("call fclose");
        // Success
        self.emit("xorl %eax, %eax");
        self.emit("xorl %edx, %edx");

        self.emit_label(".Lwf_done");
        self.emit("movq %rbp, %rsp");
        self.emit("popq %rbp");
        self.emit("ret");
        writeln!(self.output).unwrap();

        // __ferro_str_concat(left: RCX, right: RDX) -> RAX = new malloc'd string
        writeln!(self.output, "__ferro_str_concat:").unwrap();
        self.emit("pushq %rbp");
        self.emit("movq %rsp, %rbp");
        self.emit("subq $48, %rsp");
        self.emit("movq %rcx, -8(%rbp)");  // save left
        self.emit("movq %rdx, -16(%rbp)"); // save right
        // strlen(left)
        self.emit("call strlen");
        self.emit("movq %rax, -24(%rbp)"); // save left_len
        // strlen(right)
        self.emit("movq -16(%rbp), %rcx");
        self.emit("call strlen");
        self.emit("movq %rax, -32(%rbp)"); // save right_len
        // malloc(left_len + right_len + 1)
        self.emit("movq -24(%rbp), %rcx");
        self.emit("addq %rax, %rcx");
        self.emit("incq %rcx");
        self.emit("call malloc");
        self.emit("movq %rax, -40(%rbp)"); // save buffer
        // strcpy(buf, left)
        self.emit("movq %rax, %rcx");
        self.emit("movq -8(%rbp), %rdx");
        self.emit("call strcpy");
        // strcat(buf, right)
        self.emit("movq -40(%rbp), %rcx");
        self.emit("movq -16(%rbp), %rdx");
        self.emit("call strcat");
        // return buffer
        self.emit("movq -40(%rbp), %rax");
        self.emit("movq %rbp, %rsp");
        self.emit("popq %rbp");
        self.emit("ret");
        writeln!(self.output).unwrap();

        // __ferro_substr(s: RCX, start: RDX, len: R8) -> RAX = new malloc'd substring
        writeln!(self.output, "__ferro_substr:").unwrap();
        self.emit("pushq %rbp");
        self.emit("movq %rsp, %rbp");
        self.emit("subq $48, %rsp");
        self.emit("movq %rcx, -8(%rbp)");  // save s
        self.emit("movq %rdx, -16(%rbp)"); // save start
        self.emit("movq %r8, -24(%rbp)");  // save len
        // malloc(len + 1)
        self.emit("movq %r8, %rcx");
        self.emit("incq %rcx");
        self.emit("call malloc");
        self.emit("movq %rax, -32(%rbp)"); // save buffer
        // memcpy(buf, s + start, len)
        self.emit("movq %rax, %rcx");           // dest
        self.emit("movq -8(%rbp), %rdx");
        self.emit("addq -16(%rbp), %rdx");       // src = s + start
        self.emit("movq -24(%rbp), %r8");         // count = len
        self.emit("call memcpy");
        // null-terminate
        self.emit("movq -32(%rbp), %rax");
        self.emit("movq -24(%rbp), %rcx");
        self.emit("movb $0, (%rax,%rcx,1)");
        self.emit("movq %rbp, %rsp");
        self.emit("popq %rbp");
        self.emit("ret");
        writeln!(self.output).unwrap();

        // __ferro_trim(s: RCX) -> RAX = new malloc'd trimmed string
        writeln!(self.output, "__ferro_trim:").unwrap();
        self.emit("pushq %rbp");
        self.emit("movq %rsp, %rbp");
        self.emit("subq $48, %rsp");
        self.emit("movq %rcx, -8(%rbp)");  // save s
        // skip leading whitespace
        self.emit_label(".Ltrim_lskip");
        self.emit("movzbl (%rcx), %eax");
        self.emit("cmpb $32, %al");   // space
        self.emit("je .Ltrim_lnext");
        self.emit("cmpb $9, %al");    // tab
        self.emit("je .Ltrim_lnext");
        self.emit("cmpb $10, %al");   // newline
        self.emit("je .Ltrim_lnext");
        self.emit("cmpb $13, %al");   // carriage return
        self.emit("je .Ltrim_lnext");
        self.emit("jmp .Ltrim_ldone");
        self.emit_label(".Ltrim_lnext");
        self.emit("incq %rcx");
        self.emit("jmp .Ltrim_lskip");
        self.emit_label(".Ltrim_ldone");
        self.emit("movq %rcx, -16(%rbp)"); // save start ptr
        // strlen(start)
        self.emit("call strlen");
        self.emit("movq %rax, -24(%rbp)"); // save length
        // find end (skip trailing whitespace)
        self.emit("movq -16(%rbp), %rcx");
        self.emit("addq %rax, %rcx");       // end ptr
        self.emit_label(".Ltrim_rskip");
        self.emit("cmpq -16(%rbp), %rcx");
        self.emit("jle .Ltrim_rdone");
        self.emit("movzbl -1(%rcx), %eax");
        self.emit("cmpb $32, %al");
        self.emit("je .Ltrim_rnext");
        self.emit("cmpb $9, %al");
        self.emit("je .Ltrim_rnext");
        self.emit("cmpb $10, %al");
        self.emit("je .Ltrim_rnext");
        self.emit("cmpb $13, %al");
        self.emit("je .Ltrim_rnext");
        self.emit("jmp .Ltrim_rdone");
        self.emit_label(".Ltrim_rnext");
        self.emit("decq %rcx");
        self.emit("jmp .Ltrim_rskip");
        self.emit_label(".Ltrim_rdone");
        // length = end - start
        self.emit("subq -16(%rbp), %rcx");
        self.emit("movq %rcx, -24(%rbp)"); // trimmed length
        // malloc(len + 1)
        self.emit("incq %rcx");
        self.emit("call malloc");
        self.emit("movq %rax, -32(%rbp)"); // save buffer
        // memcpy(buf, start, len)
        self.emit("movq %rax, %rcx");
        self.emit("movq -16(%rbp), %rdx");
        self.emit("movq -24(%rbp), %r8");
        self.emit("call memcpy");
        // null-terminate
        self.emit("movq -32(%rbp), %rax");
        self.emit("movq -24(%rbp), %rcx");
        self.emit("movb $0, (%rax,%rcx,1)");
        self.emit("movq %rbp, %rsp");
        self.emit("popq %rbp");
        self.emit("ret");
        writeln!(self.output).unwrap();
    }

    // ── Blocks ──────────────────────────────────────────

    fn gen_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.gen_stmt(stmt);
        }
    }

    // ── Statements ──────────────────────────────────────

    fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, .. } => {
                // Special case: array literal
                if let Expr::ArrayLit { elements, .. } = value {
                    let count = elements.len();
                    self.stack_offset -= (count as i64) * 8;
                    let base = self.stack_offset;
                    self.locals.insert(name.to_string(), LocalVar {
                        offset: base,
                        ty: ValType::Int,
                        array_len: count,
                        struct_name: None,
                    });
                    for (i, elem) in elements.iter().enumerate() {
                        self.gen_expr(elem);
                        let elem_offset = base + (i as i64) * 8;
                        self.emit(&format!("movq %rax, {}(%rbp)", elem_offset));
                    }
                // Special case: struct literal
                } else if let Expr::StructLit { name: sname, fields, .. } = value {
                    let struct_info = self.struct_fields.get(sname).cloned().unwrap();
                    let count = struct_info.len();
                    self.stack_offset -= (count as i64) * 8;
                    let base = self.stack_offset;
                    self.locals.insert(name.to_string(), LocalVar {
                        offset: base,
                        ty: ValType::Int,
                        array_len: 0,
                        struct_name: Some(sname.clone()),
                    });
                    // Store each field at its correct offset
                    for (fname, fval) in fields {
                        let idx = struct_info.iter().position(|(n, _)| n == fname).unwrap();
                        self.gen_expr(fval);
                        let field_offset = base + (idx as i64) * 8;
                        self.emit(&format!("movq %rax, {}(%rbp)", field_offset));
                    }
                } else {
                    // Normal scalar let
                    let ty = self.infer_type(value);
                    self.gen_expr(value);
                    let offset = self.alloc_local(name, ty);
                    self.emit(&format!("movq %rax, {}(%rbp)", offset));
                }
            }

            Stmt::Assign { name, value, .. } => {
                self.gen_expr(value);
                let offset = self.locals[name].offset;
                self.emit(&format!("movq %rax, {}(%rbp)", offset));
            }

            Stmt::IndexAssign { object, index, value, .. } => {
                self.gen_expr(value);
                self.emit("pushq %rax");
                self.gen_expr(index);
                self.emit("movq %rax, %rcx");
                self.emit("popq %rax");
                let base = self.locals[object].offset;
                self.emit(&format!("movq %rax, {}(%rbp,%rcx,8)", base));
            }

            Stmt::FieldAssign { object, field, value, .. } => {
                self.gen_expr(value);
                let var = &self.locals[object];
                let sname = var.struct_name.as_ref().unwrap().clone();
                let base = var.offset;
                let struct_info = self.struct_fields.get(&sname).unwrap();
                let idx = struct_info.iter().position(|(n, _)| n == field).unwrap();
                let field_offset = base + (idx as i64) * 8;
                self.emit(&format!("movq %rax, {}(%rbp)", field_offset));
            }

            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    self.gen_expr(expr);
                }
                if !self.deferred.is_empty() {
                    // Save return value in aligned stack slot
                    self.emit("subq $16, %rsp");
                    self.emit("movq %rax, (%rsp)");
                    self.emit_deferred();
                    self.emit("movq (%rsp), %rax");
                    self.emit("addq $16, %rsp");
                }
                if self.current_failable {
                    self.emit("xorl %edx, %edx"); // RDX=0 = success
                }
                self.emit("movq %rbp, %rsp");
                self.emit("popq %rbp");
                self.emit("ret");
            }

            Stmt::Defer { expr, .. } => {
                self.deferred.push(expr.clone());
            }

            Stmt::Fail { message, .. } => {
                // Evaluate message string → RAX (pointer to error string)
                self.gen_expr(message);
                // Run deferred before returning error
                if !self.deferred.is_empty() {
                    self.emit("subq $16, %rsp");
                    self.emit("movq %rax, (%rsp)");
                    self.emit_deferred();
                    self.emit("movq (%rsp), %rax");
                    self.emit("addq $16, %rsp");
                }
                // Set RDX=1 to signal error, RAX already has the error string
                self.emit("movq $1, %rdx");
                self.emit("movq %rbp, %rsp");
                self.emit("popq %rbp");
                self.emit("ret");
            }

            Stmt::Expr { expr, .. } => {
                self.gen_expr(expr);
                // Result in RAX is discarded
            }

            Stmt::TailExpr { expr, .. } => {
                self.gen_expr(expr);
                if !self.deferred.is_empty() {
                    self.emit("subq $16, %rsp");
                    self.emit("movq %rax, (%rsp)");
                    self.emit_deferred();
                    self.emit("movq (%rsp), %rax");
                    self.emit("addq $16, %rsp");
                }
                if self.current_failable {
                    self.emit("xorl %edx, %edx"); // RDX=0 = success
                }
                self.emit("movq %rbp, %rsp");
                self.emit("popq %rbp");
                self.emit("ret");
            }

            Stmt::If { condition, then_block, else_block, .. } => {
                let else_label = self.new_label("else");
                let end_label = self.new_label("endif");

                // Evaluate condition → RAX
                self.gen_expr(condition);
                self.emit("testq %rax, %rax");  // check if RAX is 0
                self.emit(&format!("je {}", else_label)); // jump if false

                // Then block
                self.gen_block(then_block);
                self.emit(&format!("jmp {}", end_label));

                // Else block
                self.emit_label(&else_label);
                if let Some(else_b) = else_block {
                    self.gen_block(else_b);
                }

                self.emit_label(&end_label);
            }

            Stmt::While { condition, body, .. } => {
                let loop_label = self.new_label("while");
                let end_label = self.new_label("endwhile");

                // Push loop labels for break/continue
                self.loop_labels.push((loop_label.clone(), end_label.clone()));

                // Loop start
                self.emit_label(&loop_label);

                // Evaluate condition
                self.gen_expr(condition);
                self.emit("testq %rax, %rax");
                self.emit(&format!("je {}", end_label)); // exit if false

                // Body
                self.gen_block(body);
                self.emit(&format!("jmp {}", loop_label)); // loop back

                self.emit_label(&end_label);
                self.loop_labels.pop();
            }

            Stmt::For { var, start, end, body, .. } => {
                // Initialize loop variable
                self.gen_expr(start);
                let offset = self.alloc_local(var, ValType::Int);
                self.emit(&format!("movq %rax, {}(%rbp)", offset));

                // Evaluate end once
                self.gen_expr(end);
                let end_offset = self.alloc_local("__for_end", ValType::Int);
                self.emit(&format!("movq %rax, {}(%rbp)", end_offset));

                let loop_label = self.new_label("for");
                let cont_label = self.new_label("forcont");
                let end_label = self.new_label("endfor");
                // continue → increment (cont_label), break → end_label
                self.loop_labels.push((cont_label.clone(), end_label.clone()));

                // Loop start: check var < end
                self.emit_label(&loop_label);
                self.emit(&format!("movq {}(%rbp), %rax", offset));
                self.emit(&format!("cmpq {}(%rbp), %rax", end_offset));
                self.emit(&format!("jge {}", end_label));

                // Body
                self.gen_block(body);

                // Increment: var = var + 1 (continue target)
                self.emit_label(&cont_label);
                self.emit(&format!("movq {}(%rbp), %rax", offset));
                self.emit("addq $1, %rax");
                self.emit(&format!("movq %rax, {}(%rbp)", offset));
                self.emit(&format!("jmp {}", loop_label));

                self.emit_label(&end_label);
                self.loop_labels.pop();
            }

            Stmt::Break { .. } => {
                if let Some((_, end_label)) = self.loop_labels.last() {
                    self.emit(&format!("jmp {}", end_label.clone()));
                }
            }

            Stmt::Continue { .. } => {
                if let Some((start_label, _)) = self.loop_labels.last() {
                    self.emit(&format!("jmp {}", start_label.clone()));
                }
            }

            Stmt::Match { subject, arms, .. } => {
                // Evaluate subject once → push on stack
                self.gen_expr(subject);
                self.emit("pushq %rax");

                let end_label = self.new_label("matchend");

                for arm in arms.iter() {
                    match &arm.pattern {
                        Pattern::IntLit(v, _) => {
                            let skip = self.new_label("skip");
                            self.emit(&format!("cmpq ${}, (%rsp)", v));
                            self.emit(&format!("jne {}", skip));
                            self.emit("addq $8, %rsp"); // pop subject
                            self.gen_block(&arm.body);
                            self.emit(&format!("jmp {}", end_label));
                            self.emit_label(&skip);
                        }
                        Pattern::BoolLit(v, _) => {
                            let skip = self.new_label("skip");
                            let val: i64 = if *v { 1 } else { 0 };
                            self.emit(&format!("cmpq ${}, (%rsp)", val));
                            self.emit(&format!("jne {}", skip));
                            self.emit("addq $8, %rsp");
                            self.gen_block(&arm.body);
                            self.emit(&format!("jmp {}", end_label));
                            self.emit_label(&skip);
                        }
                        Pattern::Wildcard(_) => {
                            self.emit("addq $8, %rsp");
                            self.gen_block(&arm.body);
                            self.emit(&format!("jmp {}", end_label));
                        }
                        Pattern::EnumVariant(enum_name, variant, _) => {
                            let skip = self.new_label("skip");
                            let key = format!("{}::{}", enum_name, variant);
                            let val = self.enum_values[&key];
                            self.emit(&format!("cmpq ${}, (%rsp)", val));
                            self.emit(&format!("jne {}", skip));
                            self.emit("addq $8, %rsp");
                            self.gen_block(&arm.body);
                            self.emit(&format!("jmp {}", end_label));
                            self.emit_label(&skip);
                        }
                    }
                }

                // Fallthrough: no arm matched — clean up subject from stack
                self.emit("addq $8, %rsp");
                self.emit_label(&end_label);
            }
        }
    }

    // ── Expressions (result always ends up in RAX) ──────

    fn gen_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::IntLit { value, .. } => {
                self.emit(&format!("movq ${}, %rax", value));
            }

            Expr::BoolLit { value, .. } => {
                let v: i64 = if *value { 1 } else { 0 };
                self.emit(&format!("movq ${}, %rax", v));
            }

            Expr::StringLit { value, .. } => {
                let idx = self.string_literals.len();
                self.string_literals.push(value.clone());
                self.emit(&format!("leaq .Lstr_{}(%rip), %rax", idx));
            }

            Expr::Ident { name, span: _ } => {
                // Check if it's a comptime constant first
                if let Some(val) = self.comptime_values.get(name) {
                    self.emit(&format!("movq ${}, %rax", val));
                } else {
                    let offset = self.locals[name].offset;
                    self.emit(&format!("movq {}(%rbp), %rax", offset));
                }
            }

            Expr::UnaryOp { op, operand, .. } => {
                self.gen_expr(operand);
                match op {
                    UnaryOp::Neg => self.emit("negq %rax"),
                    UnaryOp::Not => {
                        // Logical not: if 0 → 1, if nonzero → 0
                        self.emit("testq %rax, %rax");
                        self.emit("sete %al");
                        self.emit("movzbq %al, %rax");
                    }
                }
            }

            Expr::BinaryOp { op, left, right, .. } => {
                // String concatenation: "hello" + " world"
                if *op == BinOp::Add && self.infer_type(left) == ValType::Str {
                    self.gen_expr(left);
                    self.emit("pushq %rax");     // save left string ptr
                    self.gen_expr(right);
                    self.emit("movq %rax, %rdx"); // right string in RDX
                    self.emit("popq %rcx");       // left string in RCX
                    // Call __ferro_str_concat(left, right) -> RAX = new string
                    self.emit("subq $32, %rsp");
                    self.emit("call __ferro_str_concat");
                    self.emit("addq $32, %rsp");
                    return;
                }

                // Evaluate left → RAX → push onto stack
                self.gen_expr(left);
                self.emit("pushq %rax");

                // Evaluate right → RAX
                self.gen_expr(right);
                self.emit("movq %rax, %rcx"); // right value in RCX

                // Pop left value into RAX
                self.emit("popq %rax");

                // Now: RAX = left, RCX = right
                match op {
                    BinOp::Add => self.emit("addq %rcx, %rax"),
                    BinOp::Sub => self.emit("subq %rcx, %rax"),
                    BinOp::Mul => self.emit("imulq %rcx, %rax"),
                    BinOp::Div => {
                        // idiv divides RDX:RAX by operand, quotient in RAX
                        self.emit("cqto");       // sign-extend RAX into RDX:RAX
                        self.emit("idivq %rcx");
                    }
                    BinOp::Mod => {
                        // idiv: quotient in RAX, remainder in RDX
                        self.emit("cqto");
                        self.emit("idivq %rcx");
                        self.emit("movq %rdx, %rax"); // result is remainder
                    }
                    // Comparisons: compare and set a byte, then zero-extend
                    BinOp::Eq => {
                        self.emit("cmpq %rcx, %rax");
                        self.emit("sete %al");
                        self.emit("movzbq %al, %rax");
                    }
                    BinOp::Neq => {
                        self.emit("cmpq %rcx, %rax");
                        self.emit("setne %al");
                        self.emit("movzbq %al, %rax");
                    }
                    BinOp::Lt => {
                        self.emit("cmpq %rcx, %rax");
                        self.emit("setl %al");
                        self.emit("movzbq %al, %rax");
                    }
                    BinOp::Gt => {
                        self.emit("cmpq %rcx, %rax");
                        self.emit("setg %al");
                        self.emit("movzbq %al, %rax");
                    }
                    BinOp::Lte => {
                        self.emit("cmpq %rcx, %rax");
                        self.emit("setle %al");
                        self.emit("movzbq %al, %rax");
                    }
                    BinOp::Gte => {
                        self.emit("cmpq %rcx, %rax");
                        self.emit("setge %al");
                        self.emit("movzbq %al, %rax");
                    }
                    BinOp::And => {
                        // Logical AND: both must be nonzero
                        self.emit("testq %rax, %rax");
                        self.emit("setne %al");
                        self.emit("testq %rcx, %rcx");
                        self.emit("setne %cl");
                        self.emit("andb %cl, %al");
                        self.emit("movzbq %al, %rax");
                    }
                    BinOp::Or => {
                        // Logical OR: either must be nonzero
                        self.emit("orq %rcx, %rax");
                        self.emit("testq %rax, %rax");
                        self.emit("setne %al");
                        self.emit("movzbq %al, %rax");
                    }
                }
            }

            Expr::Call { name, args, .. } => {
                // ── Built-in: print ─────────────────────────────
                if name == "print" {
                    let arg_type = self.infer_type(&args[0]);
                    self.gen_expr(&args[0]);

                    match arg_type {
                        ValType::Int => {
                            // printf("%lld\n", value)
                            self.emit("movq %rax, %rdx");
                            let fmt_idx = self.string_literals.len();
                            self.string_literals.push("%lld\\n".to_string());
                            self.emit(&format!("leaq .Lstr_{}(%rip), %rcx", fmt_idx));
                            self.emit("call printf");
                        }
                        ValType::Str => {
                            // puts(string_ptr)  — prints string + newline
                            self.emit("movq %rax, %rcx");
                            self.emit("call puts");
                        }
                        ValType::Bool => {
                            // Branch: print "true" or "false"
                            let false_label = self.new_label("pf");
                            let end_label = self.new_label("pe");
                            self.emit("testq %rax, %rax");
                            self.emit(&format!("je {}", false_label));
                            // true branch
                            let true_idx = self.string_literals.len();
                            self.string_literals.push("true".to_string());
                            self.emit(&format!("leaq .Lstr_{}(%rip), %rcx", true_idx));
                            self.emit(&format!("jmp {}", end_label));
                            // false branch
                            self.emit_label(&false_label);
                            let false_idx = self.string_literals.len();
                            self.string_literals.push("false".to_string());
                            self.emit(&format!("leaq .Lstr_{}(%rip), %rcx", false_idx));
                            self.emit_label(&end_label);
                            self.emit("call puts");
                        }
                        ValType::Void => {}
                    }
                    return;
                }

                // ── Built-in: len ───────────────────────────────
                if name == "len" {
                    // Check if arg is an array (known size at compile time)
                    if let Expr::Ident { name: arg_name, .. } = &args[0] {
                        if let Some(local) = self.locals.get(arg_name.as_str()) {
                            if local.array_len > 0 {
                                self.emit(&format!("movq ${}, %rax", local.array_len));
                                return;
                            }
                        }
                    }
                    // Otherwise, string: call strlen
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx");
                    self.emit("call strlen");
                    return;
                }

                // ── Built-in: abs(x) ───────────────────────────
                if name == "abs" {
                    self.gen_expr(&args[0]);
                    // if rax < 0, negate it
                    self.emit("movq %rax, %rcx");
                    self.emit("negq %rcx");
                    self.emit("testq %rax, %rax");
                    self.emit("cmovlq %rcx, %rax"); // if negative, use negated
                    return;
                }

                // ── Built-in: min(a, b) ────────────────────────
                if name == "min" {
                    self.gen_expr(&args[0]);
                    self.emit("pushq %rax");
                    self.gen_expr(&args[1]);
                    self.emit("popq %rcx"); // rcx = a, rax = b
                    self.emit("cmpq %rax, %rcx");
                    self.emit("cmovlq %rcx, %rax"); // if a < b, rax = a
                    return;
                }

                // ── Built-in: max(a, b) ────────────────────────
                if name == "max" {
                    self.gen_expr(&args[0]);
                    self.emit("pushq %rax");
                    self.gen_expr(&args[1]);
                    self.emit("popq %rcx"); // rcx = a, rax = b
                    self.emit("cmpq %rax, %rcx");
                    self.emit("cmovgq %rcx, %rax"); // if a > b, rax = a
                    return;
                }

                // ── Built-in: pow(base, exp) ───────────────────
                if name == "pow" {
                    // Integer power via loop: result = 1; while exp > 0 { result *= base; exp--; }
                    self.gen_expr(&args[0]);
                    self.emit("pushq %rax"); // save base
                    self.gen_expr(&args[1]);
                    self.emit("movq %rax, %rcx"); // rcx = exp
                    self.emit("popq %rdx");       // rdx = base
                    self.emit("movq $1, %rax");   // rax = result = 1
                    let loop_label = self.new_label("pow_loop");
                    let done_label = self.new_label("pow_done");
                    self.emit_label(&loop_label);
                    self.emit("testq %rcx, %rcx");
                    self.emit(&format!("jle {}", done_label));
                    self.emit("imulq %rdx, %rax"); // result *= base
                    self.emit("decq %rcx");
                    self.emit(&format!("jmp {}", loop_label));
                    self.emit_label(&done_label);
                    return;
                }

                // ── Built-in: rand() ───────────────────────────
                if name == "rand" {
                    self.emit("call rand");
                    // rand() returns int in EAX, sign-extend to 64-bit
                    self.emit("cltq");
                    return;
                }

                // ── Built-in: to_str(x) ────────────────────────
                if name == "to_str" {
                    // sprintf(buffer, "%lld", x) — allocate buffer on stack
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %r8");  // r8 = the number to convert
                    // Allocate 32 bytes on stack for the string buffer
                    self.emit("subq $32, %rsp");
                    self.emit("movq %rsp, %rcx"); // rcx = buffer pointer
                    let fmt_idx = self.string_literals.len();
                    self.string_literals.push("%lld".to_string());
                    self.emit(&format!("leaq .Lstr_{}(%rip), %rdx", fmt_idx)); // rdx = format
                    self.emit("call sprintf");
                    self.emit("movq %rsp, %rax"); // return pointer to buffer
                    // Note: buffer lives on stack — valid until function returns
                    return;
                }

                // ── Built-in: parse_int(s) ─────────────────────
                if name == "parse_int" {
                    // atoll(s) — convert string to i64, return 0 on invalid
                    // We make this failable: check if string is empty or non-numeric
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx"); // rcx = string ptr
                    self.emit("call atoll");
                    // RAX now has the parsed value, RDX = 0 (success)
                    self.emit("xorl %edx, %edx");
                    return;
                }

                // ── Built-in: char_at(s, i) ────────────────────
                if name == "char_at" {
                    self.gen_expr(&args[0]);
                    self.emit("pushq %rax"); // save string ptr
                    self.gen_expr(&args[1]);
                    self.emit("movq %rax, %rcx"); // rcx = index
                    self.emit("popq %rdx");       // rdx = string ptr
                    self.emit("movzbq (%rdx,%rcx,1), %rax"); // load byte at index
                    return;
                }

                // ── Built-in: contains(haystack, needle) ───────
                if name == "contains" {
                    self.gen_expr(&args[0]);
                    self.emit("pushq %rax");
                    self.gen_expr(&args[1]);
                    self.emit("movq %rax, %rdx"); // rdx = needle
                    self.emit("popq %rcx");       // rcx = haystack
                    self.emit("call strstr");
                    // strstr returns NULL (0) if not found, pointer if found
                    self.emit("testq %rax, %rax");
                    self.emit("setne %al");
                    self.emit("movzbq %al, %rax");
                    return;
                }

                // ── Built-in: starts_with(s, prefix) ──────────
                if name == "starts_with" {
                    self.gen_expr(&args[1]);
                    self.emit("pushq %rax"); // save prefix
                    // Get length of prefix
                    self.emit("movq %rax, %rcx");
                    self.emit("call strlen");
                    self.emit("pushq %rax"); // save prefix_len
                    // Call strncmp(s, prefix, prefix_len)
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx"); // rcx = s
                    self.emit("popq %r8");        // r8 = prefix_len
                    self.emit("popq %rdx");       // rdx = prefix
                    self.emit("call strncmp");
                    // strncmp returns 0 if equal
                    self.emit("testq %rax, %rax");
                    self.emit("sete %al");
                    self.emit("movzbq %al, %rax");
                    return;
                }

                // ── Built-in: read_line() ──────────────────────
                if name == "read_line" {
                    // fgets(buffer, size, stdin) — read line from stdin
                    // Allocate 1024-byte buffer on stack
                    self.emit("subq $1024, %rsp");
                    self.emit("movq %rsp, %rcx");     // rcx = buffer
                    self.emit("movq $1024, %rdx");     // rdx = size
                    // Get stdin: __acrt_iob_func(0) on Windows
                    self.emit("pushq %rcx");
                    self.emit("pushq %rdx");
                    self.emit("xorl %ecx, %ecx");      // 0 = stdin
                    self.emit("call __acrt_iob_func");
                    self.emit("movq %rax, %r8");        // r8 = stdin FILE*
                    self.emit("popq %rdx");
                    self.emit("popq %rcx");
                    self.emit("call fgets");
                    // Strip trailing newline if present
                    self.emit("movq %rsp, %rcx");       // rcx = buffer
                    self.emit("call strlen");
                    self.emit("testq %rax, %rax");
                    let skip_label = self.new_label("rl_skip");
                    self.emit(&format!("jz {}", skip_label));
                    self.emit("decq %rax");
                    self.emit("movq %rsp, %rcx");
                    self.emit("cmpb $10, (%rcx,%rax,1)"); // check for '\n'
                    let no_nl_label = self.new_label("rl_no_nl");
                    self.emit(&format!("jne {}", no_nl_label));
                    self.emit("movb $0, (%rcx,%rax,1)"); // replace '\n' with '\0'
                    self.emit_label(&no_nl_label);
                    // Also strip '\r' if present (Windows CRLF)
                    self.emit("testq %rax, %rax");
                    let skip_cr_label = self.new_label("rl_skip_cr");
                    self.emit(&format!("jz {}", skip_cr_label));
                    self.emit("decq %rax");
                    self.emit("cmpb $13, (%rcx,%rax,1)"); // check for '\r'
                    let no_cr_label = self.new_label("rl_no_cr");
                    self.emit(&format!("jne {}", no_cr_label));
                    self.emit("movb $0, (%rcx,%rax,1)");
                    self.emit_label(&no_cr_label);
                    self.emit_label(&skip_cr_label);
                    self.emit_label(&skip_label);
                    self.emit("movq %rsp, %rax"); // return buffer pointer
                    return;
                }

                // ── Built-in: read_file(path) ──────────────────
                if name == "read_file" {
                    // Calls __ferro_read_file helper (emitted at end of assembly)
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx");
                    self.emit("call __ferro_read_file");
                    // Returns: RAX = string ptr (or error msg), RDX = 0 (ok) or 1 (error)
                    return;
                }

                // ── Built-in: write_file(path, content) ────────
                if name == "write_file" {
                    // Calls __ferro_write_file helper
                    self.gen_expr(&args[0]);
                    self.emit("pushq %rax");
                    self.gen_expr(&args[1]);
                    self.emit("movq %rax, %rdx"); // rdx = content
                    self.emit("popq %rcx");       // rcx = path
                    self.emit("call __ferro_write_file");
                    // Returns: RDX = 0 (ok) or 1 (error), RAX = error msg if error
                    return;
                }

                // ── Built-in: file_exists(path) ────────────────
                if name == "file_exists" {
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx"); // path
                    let mode_idx = self.string_literals.len();
                    self.string_literals.push("r".to_string());
                    self.emit(&format!("leaq .Lstr_{}(%rip), %rdx", mode_idx));
                    self.emit("call fopen");
                    self.emit("testq %rax, %rax");
                    let exists_label = self.new_label("fe_yes");
                    let done_label = self.new_label("fe_done");
                    self.emit(&format!("jnz {}", exists_label));
                    self.emit("xorl %eax, %eax"); // false
                    self.emit(&format!("jmp {}", done_label));
                    self.emit_label(&exists_label);
                    // Close the file we opened
                    self.emit("movq %rax, %rcx");
                    self.emit("call fclose");
                    self.emit("movq $1, %rax"); // true
                    self.emit_label(&done_label);
                    return;
                }

                // ── Built-in: eprint(s) ────────────────────────
                if name == "eprint" {
                    self.gen_expr(&args[0]);
                    // fprintf(stderr, "%s\n", s)
                    self.emit("movq %rax, %r8");  // r8 = string
                    let fmt_idx = self.string_literals.len();
                    self.string_literals.push("%s\\n".to_string());
                    self.emit(&format!("leaq .Lstr_{}(%rip), %rdx", fmt_idx)); // format
                    // Get stderr: __acrt_iob_func(2) on Windows
                    self.emit("pushq %rdx");
                    self.emit("pushq %r8");
                    self.emit("movq $2, %rcx");
                    self.emit("call __acrt_iob_func");
                    self.emit("movq %rax, %rcx"); // rcx = stderr
                    self.emit("popq %r8");
                    self.emit("popq %rdx");
                    self.emit("call fprintf");
                    return;
                }

                // ── Built-in: exit(code) ───────────────────────
                if name == "exit" {
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx");
                    self.emit("call exit");
                    return;
                }

                // ── Built-in: time() ───────────────────────────
                if name == "time" {
                    self.emit("xorl %ecx, %ecx"); // NULL arg
                    self.emit("call time");
                    return;
                }

                // ── Built-in: sleep(ms) ────────────────────────
                if name == "sleep" {
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx");
                    self.emit("call Sleep"); // Windows API
                    return;
                }

                // ── substr(s, start, len) ──────────────
                if name == "substr" {
                    // Evaluate args: s, start, len
                    self.gen_expr(&args[0]);
                    self.emit("pushq %rax");
                    self.gen_expr(&args[1]);
                    self.emit("pushq %rax");
                    self.gen_expr(&args[2]);
                    self.emit("movq %rax, %r8");    // len in R8
                    self.emit("popq %rdx");          // start in RDX
                    self.emit("popq %rcx");          // s in RCX
                    self.emit("subq $32, %rsp");
                    self.emit("call __ferro_substr");
                    self.emit("addq $32, %rsp");
                    return;
                }

                // ── trim(s) ────────────────────────────
                if name == "trim" {
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx");
                    self.emit("subq $32, %rsp");
                    self.emit("call __ferro_trim");
                    self.emit("addq $32, %rsp");
                    return;
                }

                // ── alloc(size) → malloc ───────────────
                if name == "alloc" {
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx");
                    self.emit("call malloc");
                    return;
                }

                // ── free(ptr) → free ───────────────────
                if name == "free" {
                    self.gen_expr(&args[0]);
                    self.emit("movq %rax, %rcx");
                    self.emit("call free");
                    return;
                }

                // General function call
                let arg_regs = ["%rcx", "%rdx", "%r8", "%r9"];

                // Evaluate arguments and push them (right-to-left for safety)
                // But we need them in registers, so evaluate left-to-right
                // and save to stack temporarily
                let mut arg_offsets = Vec::new();
                for arg in args {
                    self.gen_expr(arg);
                    self.emit("pushq %rax");
                    arg_offsets.push(());
                }

                // Pop into the correct registers (reverse order)
                for i in (0..args.len()).rev() {
                    if i < arg_regs.len() {
                        self.emit(&format!("popq {}", arg_regs[i]));
                    } else {
                        self.emit("popq %rax"); // discard (stack args not yet supported)
                    }
                }

                self.emit(&format!("call {}", name));
                // Result is in RAX
            }

            Expr::ArrayLit { .. } => {
                // Array literals are handled in gen_stmt(Let) — not used standalone
            }

            Expr::Index { object, index, .. } => {
                self.gen_expr(index);
                if let Expr::Ident { name, .. } = object.as_ref() {
                    let base = self.locals[name].offset;
                    self.emit(&format!("movq {}(%rbp,%rax,8), %rax", base));
                }
            }

            Expr::EnumVariant { enum_name, variant, .. } => {
                let key = format!("{}::{}", enum_name, variant);
                let val = self.enum_values[&key];
                self.emit(&format!("movq ${}, %rax", val));
            }

            Expr::StructLit { .. } => {
                // Struct literals are handled in gen_stmt(Let)
            }

            Expr::FieldAccess { object, field, .. } => {
                if let Expr::Ident { name, .. } = object.as_ref() {
                    let var = &self.locals[name];
                    let sname = var.struct_name.as_ref().unwrap().clone();
                    let base = var.offset;
                    let struct_info = self.struct_fields.get(&sname).unwrap();
                    let idx = struct_info.iter().position(|(n, _)| n == field).unwrap();
                    let field_offset = base + (idx as i64) * 8;
                    self.emit(&format!("movq {}(%rbp), %rax", field_offset));
                }
            }

            Expr::Try { expr, .. } => {
                // Generate the failable function call
                self.gen_expr(expr);
                // After call: RAX = return value, RDX = error flag
                // If RDX != 0, propagate the error (return with same RAX/RDX)
                let ok_label = self.new_label("try_ok");
                self.emit("testq %rdx, %rdx");
                self.emit(&format!("je {}", ok_label));
                // Error path: propagate — run deferred, then return with error
                if !self.deferred.is_empty() {
                    self.emit("subq $16, %rsp");
                    self.emit("movq %rax, (%rsp)");
                    self.emit_deferred();
                    self.emit("movq (%rsp), %rax");
                    self.emit("addq $16, %rsp");
                    self.emit("movq $1, %rdx"); // re-set error flag after deferred
                }
                self.emit("movq %rbp, %rsp");
                self.emit("popq %rbp");
                self.emit("ret");
                // Success path: RAX has the unwrapped value
                self.emit_label(&ok_label);
            }

            Expr::Cast { expr, target, .. } => {
                self.gen_expr(expr);
                let from = self.infer_type(expr);
                match (from, target.as_str()) {
                    // i64 → bool: 0=false, nonzero=true
                    (ValType::Int, "bool") => {
                        self.emit("testq %rax, %rax");
                        self.emit("setne %al");
                        self.emit("movzbq %al, %rax");
                    }
                    // bool → i64: already 0 or 1, no-op
                    (ValType::Bool, "i64") => {}
                    // i64 → str: malloc buffer + sprintf
                    (ValType::Int, "str") => {
                        self.emit("pushq %rax");  // save value at (%rsp)
                        // malloc(24) — enough for i64 as string
                        self.emit("movq $24, %rcx");
                        self.emit("subq $32, %rsp"); // shadow space for malloc
                        self.emit("call malloc");
                        self.emit("addq $32, %rsp");
                        // RAX = buf ptr, (%rsp) = original value
                        self.emit("pushq %rax");      // save buf ptr at (%rsp), value at 8(%rsp)
                        self.emit("movq %rax, %rcx"); // buf in RCX (arg 1)
                        let fmt_idx = self.string_literals.len();
                        self.string_literals.push("%lld".to_string());
                        self.emit(&format!("leaq .Lstr_{}(%rip), %rdx", fmt_idx));
                        self.emit("movq 8(%rsp), %r8"); // original i64 value
                        self.emit("subq $32, %rsp");
                        self.emit("call sprintf");
                        self.emit("addq $32, %rsp");
                        self.emit("popq %rax");   // buf ptr = result
                        self.emit("addq $8, %rsp"); // discard saved value
                    }
                    // str → i64: atoll
                    (ValType::Str, "i64") => {
                        self.emit("movq %rax, %rcx");
                        self.emit("call atoll");
                    }
                    _ => {} // other casts — no-op (sema already validated)
                }
            }
        }
    }
}

/// Count the total number of local variables in a function (params + lets).
fn count_locals(func: &Function) -> usize {
    func.params.len() + count_locals_in_block(&func.body)
}

fn count_locals_in_block(block: &Block) -> usize {
    let mut count = 0;
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { value, .. } => {
                if let Expr::ArrayLit { elements, .. } = value {
                    count += elements.len();
                } else if let Expr::StructLit { fields, .. } = value {
                    count += fields.len();
                } else {
                    count += 1;
                }
            }
            Stmt::If { then_block, else_block, .. } => {
                count += count_locals_in_block(then_block);
                if let Some(eb) = else_block {
                    count += count_locals_in_block(eb);
                }
            }
            Stmt::While { body, .. } => {
                count += count_locals_in_block(body);
            }
            Stmt::For { body, .. } => {
                count += 2; // loop variable + __for_end
                count += count_locals_in_block(body);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    count += count_locals_in_block(&arm.body);
                }
            }
            _ => {}
        }
    }
    count
}

/// Round up to the nearest multiple of 16.
fn align16(n: i64) -> i64 {
    (n + 15) & !15
}
