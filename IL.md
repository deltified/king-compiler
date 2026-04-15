### The IL Draft

#### **1. Types & Structure**
*   **Types:** `i8`, `i32`, `i64`, `ptr`, `void` (only for function returns).
*   **Structure:** `Module` contains `Functions`. `Functions` contain a CFG of `Basic Blocks`. `Basic Blocks` contain `Instructions`.
*   **Registers:** Denoted by `%`, e.g., `%0`, `%v_name`.
*   **Labels:** Denoted by `.`, e.g., `.block1`.

#### **2. Instruction Set Architecture (ISA)**

**Arithmetic & Logic (Pure, no side effects)**
```text
%res = add i32 %a, %b
%res = sub i64 %a, %b
%res = mul i32 %a, %b
%res = sdiv i32 %a, %b   // Signed division
%res = and i8 %a, %b
```

**Memory (Side-effecting)**
```text
// Allocate space on the stack (returns a ptr)
%ptr = alloca i32

// Store a value into a pointer
store i32 %val, ptr %ptr

// Load a value from a pointer
%val = load i32, ptr %ptr
```

**Control Flow (CFG Terminators)**
*Note: Every basic block MUST end with a terminator.*
```text
// Unconditional jump
jmp .block2

// Conditional branch (icmp returns an i8 acting as a boolean 1 or 0)
%cond = icmp eq i32 %a, %b
br i8 %cond, .block_true, .block_false

// Return from function
ret i32 %val
ret void
```

**Functions & SSA**
```text
// Call a function (Side-effecting)
%res = call i32 @my_func(i32 %arg1, ptr %arg2)

// Phi node (Crucial for SSA: chooses a value based on where the control flow came from)
%val = phi i32 [ %v1, .block1 ], [ %v2, .block2 ]
```

**Metadata (Hints)**
Metadata can be attached to the end of instructions using `!`.
```text
%ptr = load ptr, ptr %base !nonnull !align(8)
```

#### **3. Example: Factorial Function in IL**

Here is what a `while`-loop based factorial function looks like in this IL. Notice the strict SSA form (variables are assigned exactly once) and the use of `phi` nodes at the start of loop blocks.

```text
func @factorial(i32 %n) -> i32 {
.entry:
    // Is n <= 1?
    %is_base = icmp sle i32 %n, 1
    br i8 %is_base, .end, .loop_header

.loop_header:
    // In SSA, loop variables need phi nodes to merge 
    // the initial value and the value from the previous loop iteration.
    %current_n = phi i32 [ %n, .entry ], [ %next_n, .loop_body ]
    %acc       = phi i32 [ 1, .entry ], [ %next_acc, .loop_body ]
    
    // Check loop condition
    %cond = icmp sgt i32 %current_n, 1
    br i8 %cond, .loop_body, .end

.loop_body:
    %next_acc = mul i32 %acc, %current_n
    %next_n   = sub i32 %current_n, 1
    jmp .loop_header

.end:
    // Merge the return value from the base case or the loop finish
    %result = phi i32 [ 1, .entry ], [ %acc, .loop_header ]
    ret i32 %result
}
```