.text
.globl _add2
.p2align 2
_add2:
bb0_entry:
    add x0, x0, x1
    ret

.text
.globl _main
.p2align 2
_main:
    str x30, [sp, #-16]!
bb0_entry:
    mov x0, #40
    mov x1, #2
    bl _add2
    ldr x30, [sp], #16
    ret
