
# COMPLETED:
```
### Phase 0: Project Setup & Data Structures
Before writing compiler logic, we must establish the data structures. If we were to use standard Rust references (`&` or `Box`) for the Control Flow Graph (CFG), we would enter "Borrow Checker Hell." 

1.  **Dependencies:** 
    *   Add `slotmap` or `id-arena` (for storing instructions/blocks without lifetimes).
    *   Add `petgraph` (for managing the CFG).
2.  **Arena Setup:**
    *   Define strongly typed IDs: `InstrId`, `BlockId`, `ValueId`.
3.  **Basic Types:**
    *   Define the `Type` enum (`I8`, `I32`, `I64`, `Ptr`, `Void`).

```
# TODO:
---

### Phase 1: Machine IR (MIR) & Assembly Emission
**Goal:** Create a representation of actual CPU instructions and write them to a text file that a standard assembler (like `gcc` or `nasm`) can compile. *We are targeting a specific architecture here (e.g., x86_64).*

Write this for arm64 first, then for x86_64.

1.  **Define Physical Registers:** Create an enum for the target's actual registers (e.g., `RAX`, `RSP`, `RDI`).
2.  **Define the MIR Instruction Enum:**
    *   Needs to support *both* physical registers and infinite virtual registers (e.g., `VReg(usize)`).
    *   Include instructions like `Mov`, `Add`, `Sub`, `Call`, `Cmp`, `Jmp`, `JmpIf`.
3.  **Write the ASM Printer:**
    *   Create a pass that takes a list of MIR instructions and formats them into an Assembly string.
    *   *Test:* Manually hardcode a MIR sequence that puts `42` into `RAX` and calls `Ret`. Pass it to the printer, save to `test.s`, run `gcc test.s -o test`, and check the exit code (`echo $?` should be 42).

---

### Phase 2: Register Allocation & Stack Frames
**Goal:** Bridge the gap between infinite virtual registers and finite physical CPU registers.

1.  **Function Prologue/Epilogue Insertion:**
    *   Write a pass that wraps MIR functions with stack frame setups (e.g., `push rbp; mov rbp, rsp` on x86).
2.  **Liveness Analysis (Simple):**
    *   Figure out where a virtual register is first defined and where it is last used.
3.  **Naive Register Allocation (Linear Scan or Spill-Everything):**
    *   *V1 (Spill-Everything):* The easiest allocator. Assign *every* virtual register a slot on the stack. To do `add %v1, %v2`, load them from the stack into physical registers (like `RAX` and `RCX`), add them, and store the result back to the stack.
    *   *V2 (Linear Scan):* Map virtual registers to physical ones until you run out, *then* spill to the stack.
4.  **Handling Clobbers:**
    *   When the allocator hits a `Call` MIR instruction, force it to evict all caller-saved registers to the stack.
    *   *Test:* Hand-write MIR with 30 virtual registers. Run the allocator. Check the emitted ASM to ensure stack offsets (`[rbp - 8]`) are used correctly.

---

### Phase 3: High-Level IL Construction
**Goal:** Build the in-memory representation of the platform-independent language we drafted previously.

1.  **Define the IL Enums:**
    *   Create `Instruction` (`Add`, `Load`, `Store`, `Br`, `Phi`, etc.).
    *   Add the `has_side_effects(&self) -> bool` method.
2.  **Build the CFG Structure:**
    *   Create the `BasicBlock` struct (contains a list of `InstrId`).
    *   Create the `Function` struct (contains the Arena of instructions, blocks, and the `petgraph` representing jumps between blocks).
3.  **Create the IR Builder API:**
    *   Write a fluent Rust API to construct IL easily. The API should be straightforward (relatively speaking).
    *   Example: `builder.build_add(val1, val2)` which automatically inserts the instruction into the current basic block and returns a new `ValueId`.
    *   *Test:* Write Rust code to build the "Factorial" IL. Write a simple string-formatter that prints the IL to the console so we can visually verify it.

---

### Phase 4: Instruction Selection (Lowering) & SSA Deconstruction
**Goal:** Translate the platform-independent High-Level IL into the architecture-specific MIR.

1.  **SSA Deconstruction (Phi Node Elimination):**
    *   Hardware doesn't understand `phi` nodes. 
    *   Write a pass that removes `phi` nodes by inserting `mov` instructions into the predecessor basic blocks. (e.g., if `%val = phi [%a, .B1], [%b, .B2]`, insert `%val = %a` at the end of block `.B1`, etc.)
2.  **Instruction Lowering Pass:**
    *   Iterate through High-Level IL instructions and map them to MIR.
    *   `add i32 %1, %2` -> MIR `Add(VReg(1), VReg(2))`.
    *   `store i32 %val, ptr %ptr` -> MIR `MovToMem(VReg(ptr), VReg(val))`.
3.  **End-to-End Test:**
    *   **The Golden Milestone.** Use your Builder to make an IL function. Lower it -> Allocate Registers -> Emit ASM -> Compile with GCC -> Run. *We now have a fully working, albeit naive, compiler backend!*

---

### Phase 5: The Optimization Pipeline
**Goal:** Now that the naive path works perfectly, we can transform "Naive IL" into "Optimized IL" *before* lowering it.

Because we are using an Arena, optimizations usually work by creating a new, empty Function and copying instructions over, omitting or modifying them as needed, rather than mutating the Arena in place.

1.  **Dead Code Elimination (DCE):**
    *   Traverse instructions backwards. Keep a `HashSet` of `ValueId`s that are used.
    *   If an instruction produces a `ValueId` that isn't in the set, AND `has_side_effects()` is false, delete it.
2.  **Constant Folding:**
    *   Iterate through instructions. If you see `add i32 5, 10`, replace all uses of that instruction's ID with a constant `15`, and delete the instruction.
3.  **CFG Simplification:**
    *   If Block A unconditionally jumps to Block B, and Block B has no other incoming edges, merge them into a single Basic Block.
    *   *Test:* Write unit tests for each pass. Assert that feeding `add 5, 5` into the CFG results in an IL graph that only contains `10`.

---

### Phase 6: Emitting Real Binaries (Future Polish)
**Goal:** Remove the dependency on `gcc` or `nasm` to assemble the text files.

1.  **The `object` Crate:**
    *   Include the `object` crate in Rust.
    *   Instead of formatting MIR into a `String`, map MIR instructions directly to their raw hexadecimal opcodes (e.g., `MOV RAX, RBX` -> `0x48 0x89 0xD8`).
    *   Use the `object` crate to wrap these bytes into an ELF (Linux), Mach-O (macOS), or PE (Windows) file.
    *   *Note:* This is highly rewarding but tedious, which is why Step 1 relies on text-assembly.

---

### Summary of Data Flow
The compiler's `main.rs` will eventually look like this:

```rust
// 1. Frontend gives us Naive IL (or we build it manually)
let mut il_func = build_factorial_il();

// 2. High-Level Optimizations
il_func = passes::constant_fold(il_func);
il_func = passes::dead_code_elimination(il_func);

// 3. SSA Deconstruction
il_func = passes::eliminate_phi_nodes(il_func);

// 4. Instruction Selection
let mut mir_func = lower_il_to_mir(il_func);

// 5. Register Allocation
let allocated_mir = register_allocation::linear_scan(mir_func);

// 6. Assembly Emission
let asm_string = emit_x86_64_assembly(allocated_mir);
std::fs::write("output.s", asm_string);
```

