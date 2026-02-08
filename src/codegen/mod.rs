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
        self.locals.insert(name.to_string(), LocalVar { offset, ty, array_len: 0 });
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
            Expr::BinaryOp { op, .. } => match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => ValType::Int,
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

    // ── Program ─────────────────────────────────────────

    /// Generate assembly for an entire program.
    pub fn generate(mut self, program: &Program) -> String {
        // Collect function return types so infer_type can resolve Call exprs.
        for func in &program.items {
            let rt = match &func.return_type {
                Some(n) => Self::valtype_from_name(n),
                None => ValType::Void,
            };
            self.func_return_types.insert(func.name.clone(), rt);
        }
        // Emit text section
        writeln!(self.output, "    .section .text").unwrap();

        for func in &program.items {
            self.gen_function(func);
        }

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
        // Reset locals for each function
        self.locals.clear();
        self.stack_offset = 0;

        // Count how many locals we need (params + local vars in body)
        let local_count = count_locals(func);
        // Stack space: locals * 8, rounded up to 16-byte alignment
        // Plus 32 bytes shadow space for any calls we make
        let frame_size = align16((local_count as i64) * 8 + 32);

        // Function label
        writeln!(self.output, "    .globl {}", func.name).unwrap();
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

        // If we fall through without a return, return 0
        self.emit("xorl %eax, %eax");

        // Epilogue
        self.emit_label(&format!(".L{}_epilogue", func.name));
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
                // Special case: array literal — allocate N slots and init each
                if let Expr::ArrayLit { elements, .. } = value {
                    let count = elements.len();
                    // Reserve count * 8 bytes on the stack
                    self.stack_offset -= (count as i64) * 8;
                    let base = self.stack_offset;
                    self.locals.insert(name.to_string(), LocalVar {
                        offset: base,
                        ty: ValType::Int, // element type (all arrays are i64 for now)
                        array_len: count,
                    });
                    // Initialize each element
                    for (i, elem) in elements.iter().enumerate() {
                        self.gen_expr(elem);
                        let elem_offset = base + (i as i64) * 8;
                        self.emit(&format!("movq %rax, {}(%rbp)", elem_offset));
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
                // Evaluate value → push
                self.gen_expr(value);
                self.emit("pushq %rax");
                // Evaluate index → RAX
                self.gen_expr(index);
                self.emit("movq %rax, %rcx");   // RCX = index
                // Pop value → RAX
                self.emit("popq %rax");
                // Store at base + index*8
                let base = self.locals[object].offset;
                self.emit(&format!("movq %rax, {}(%rbp,%rcx,8)", base));
            }

            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    self.gen_expr(expr);
                } else {
                    self.emit("xorl %eax, %eax");
                }
                // Jump to epilogue (which restores stack and returns)
                // We need to know the function name — use a convention
                self.emit("movq %rbp, %rsp");
                self.emit("popq %rbp");
                self.emit("ret");
            }

            Stmt::Expr { expr, .. } => {
                self.gen_expr(expr);
                // Result in RAX is discarded
            }

            Stmt::TailExpr { expr, .. } => {
                // Implicit return: evaluate expression, result stays in RAX
                // Then jump to function epilogue
                self.gen_expr(expr);
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
                let offset = self.locals[name].offset;
                self.emit(&format!("movq {}(%rbp), %rax", offset));
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
                // Evaluate index → RAX
                self.gen_expr(index);
                // Load from base + index*8
                if let Expr::Ident { name, .. } = object.as_ref() {
                    let base = self.locals[name].offset;
                    self.emit(&format!("movq {}(%rbp,%rax,8), %rax", base));
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
                // Arrays need N stack slots, scalars need 1
                if let Expr::ArrayLit { elements, .. } = value {
                    count += elements.len();
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
            _ => {}
        }
    }
    count
}

/// Round up to the nearest multiple of 16.
fn align16(n: i64) -> i64 {
    (n + 15) & !15
}
