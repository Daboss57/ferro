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

/// Code generator state.
pub struct Codegen {
    /// The output assembly string.
    output: String,
    /// Counter for generating unique labels (for if/else, while, etc.)
    label_counter: usize,
    /// String literals collected during codegen, emitted in .data section.
    string_literals: Vec<String>,
    /// Maps variable name → stack offset from RBP (negative).
    locals: HashMap<String, i64>,
    /// Next available stack offset for a new local variable.
    stack_offset: i64,
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            label_counter: 0,
            string_literals: Vec::new(),
            locals: HashMap::new(),
            stack_offset: 0,
        }
    }

    /// Generate a unique label name.
    fn new_label(&mut self, prefix: &str) -> String {
        self.label_counter += 1;
        format!(".L{}_{}", prefix, self.label_counter)
    }

    /// Allocate a local variable on the stack, return its RBP offset.
    fn alloc_local(&mut self, name: &str) -> i64 {
        self.stack_offset -= 8;
        let offset = self.stack_offset;
        self.locals.insert(name.to_string(), offset);
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

    // ── Program ─────────────────────────────────────────

    /// Generate assembly for an entire program.
    pub fn generate(mut self, program: &Program) -> String {
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
            let offset = self.alloc_local(&param.name);
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
                // Evaluate the value → RAX
                self.gen_expr(value);
                // Allocate stack slot and store
                let offset = self.alloc_local(name);
                self.emit(&format!("movq %rax, {}(%rbp)", offset));
            }

            Stmt::Assign { name, value, .. } => {
                self.gen_expr(value);
                let offset = self.locals[name];
                self.emit(&format!("movq %rax, {}(%rbp)", offset));
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
                let offset = self.locals[name];
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
                // Built-in: print — calls printf with "%lld\n" format for ints
                if name == "print" {
                    self.gen_expr(&args[0]);
                    // Move value to RDX (2nd arg), format string to RCX (1st arg)
                    self.emit("movq %rax, %rdx");
                    let fmt_idx = self.string_literals.len();
                    self.string_literals.push("%lld\\n".to_string());
                    self.emit(&format!("leaq .Lstr_{}(%rip), %rcx", fmt_idx));
                    // Shadow space (32 bytes) is already reserved in our frame
                    self.emit("call printf");
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
            Stmt::Let { .. } => count += 1,
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
