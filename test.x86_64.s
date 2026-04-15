.text
.globl _add2
_add2:
    pushq %rbp
    movq %rsp, %rbp
bb0_entry:
    movq %rdi, %rcx
    movq %rsi, %rsi
    movq %rcx, %rdi
    addq %rsi, %rdi
    movq %rdi, %rax
    popq %rbp
    ret

.text
.globl _main
_main:
    pushq %rbp
    movq %rsp, %rbp
    subq $16, %rsp
bb0_entry:
    movq $40, %rcx
    movq %rcx, -8(%rbp)
    movq $2, %rcx
    movq %rcx, -16(%rbp)
    movq -8(%rbp), %rdi
    movq -16(%rbp), %rsi
    call _add2
    movq %rax, %rcx
    movq %rcx, %rax
    addq $16, %rsp
    popq %rbp
    ret
