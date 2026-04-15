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
    mov x16, x29
    sub sp, sp, #16
    add x29, sp, #16
    str x16, [x29, #-8]
    str x30, [x29, #-16]
bb0_entry:
    mov x0, #40
    mov x1, #2
    bl _add2
    ldr x30, [x29, #-16]
    ldr x29, [x29, #-8]
    add sp, sp, #16
    ret
