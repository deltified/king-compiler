.text
.globl _add2
.p2align 2
_add2:
    mov x16, x29
    sub sp, sp, #48
    add x29, sp, #48
    str x16, [x29, #-32]
    str x30, [x29, #-40]
bb0_entry:
    str x0, [x29, #-8]
    str x1, [x29, #-16]
    ldr x17, [x29, #-8]
    str x17, [x29, #-24]
    ldr x16, [x29, #-24]
    ldr x17, [x29, #-16]
    add x15, x16, x17
    str x15, [x29, #-24]
    ldr x17, [x29, #-24]
    mov x0, x17
    ldr x30, [x29, #-40]
    ldr x29, [x29, #-32]
    add sp, sp, #48
    ret

.text
.globl _main
.p2align 2
_main:
    mov x16, x29
    sub sp, sp, #64
    add x29, sp, #64
    str x16, [x29, #-48]
    str x30, [x29, #-56]
bb0_entry:
    mov x16, #40
    str x16, [x29, #-24]
    ldr x16, [x29, #-24]
    str x16, [x29, #-8]
    mov x16, #2
    str x16, [x29, #-32]
    ldr x16, [x29, #-32]
    str x16, [x29, #-16]
    ldr x0, [x29, #-8]
    ldr x1, [x29, #-16]
    bl _add2
    str x0, [x29, #-40]
    ldr x17, [x29, #-40]
    mov x0, x17
    ldr x30, [x29, #-56]
    ldr x29, [x29, #-48]
    add sp, sp, #64
    ret
