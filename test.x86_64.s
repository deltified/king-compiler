.text
.globl _add2
_add2:
bb0_entry:
    movq %rdi, %rcx
    movq %rcx, %rdi
    addq %rsi, %rdi
    movq %rdi, %rax
    ret

.text
.globl _main
_main:
    pushq %rbp
    movq %rsp, %rbp
bb0_entry:
    movq $40, %rdi
    movq $2, %rsi
    call _add2
    popq %rbp
    ret
