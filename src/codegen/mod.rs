// Code generation — emits x86-64 assembly.

/// Generates a hardcoded "hello world" x86-64 assembly program (Windows x64, MinGW/GCC).
/// This is a Phase 1 proof-of-concept — real codegen comes in Phase 5.
pub fn emit_hello_world() -> String {
    // Uses puts() which is simpler and more reliable than printf on MinGW
    r#"    .section .data
msg:
    .ascii "Hello from Ferro!\0"

    .section .text
    .globl main
main:
    subq $40, %rsp          # 32 bytes shadow space + 8 for alignment
    leaq msg(%rip), %rcx    # first arg = pointer to string
    call puts
    xorl %eax, %eax         # return 0
    addq $40, %rsp
    ret
"#
    .to_string()
}
