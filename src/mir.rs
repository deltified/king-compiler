use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetArch {
    Arm64,
    Amd64,
    X86_64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysReg {
    // arm64 general-purpose registers
    X0,
    X1,
    X2,
    X3,
    X4,
    X5,
    X6,
    X7,
    X8,
    X9,
    X10,
    X11,
    X12,
    X13,
    X14,
    X15,
    X16,
    X17,
    X18,
    X19,
    X20,
    X21,
    X22,
    X23,
    X24,
    X25,
    X26,
    X27,
    X28,
    X29,
    X30,
    SP,

    // x86_64 general-purpose registers
    RAX,
    RBX,
    RCX,
    RDX,
    RSI,
    RDI,
    RBP,
    RSP,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

impl PhysReg {
    fn as_arm64_name(self) -> Option<&'static str> {
        match self {
            Self::X0 => Some("x0"),
            Self::X1 => Some("x1"),
            Self::X2 => Some("x2"),
            Self::X3 => Some("x3"),
            Self::X4 => Some("x4"),
            Self::X5 => Some("x5"),
            Self::X6 => Some("x6"),
            Self::X7 => Some("x7"),
            Self::X8 => Some("x8"),
            Self::X9 => Some("x9"),
            Self::X10 => Some("x10"),
            Self::X11 => Some("x11"),
            Self::X12 => Some("x12"),
            Self::X13 => Some("x13"),
            Self::X14 => Some("x14"),
            Self::X15 => Some("x15"),
            Self::X16 => Some("x16"),
            Self::X17 => Some("x17"),
            Self::X18 => Some("x18"),
            Self::X19 => Some("x19"),
            Self::X20 => Some("x20"),
            Self::X21 => Some("x21"),
            Self::X22 => Some("x22"),
            Self::X23 => Some("x23"),
            Self::X24 => Some("x24"),
            Self::X25 => Some("x25"),
            Self::X26 => Some("x26"),
            Self::X27 => Some("x27"),
            Self::X28 => Some("x28"),
            Self::X29 => Some("x29"),
            Self::X30 => Some("x30"),
            Self::SP => Some("sp"),
            _ => None,
        }
    }

    fn as_x86_64_name(self) -> Option<&'static str> {
        match self {
            Self::RAX => Some("%rax"),
            Self::RBX => Some("%rbx"),
            Self::RCX => Some("%rcx"),
            Self::RDX => Some("%rdx"),
            Self::RSI => Some("%rsi"),
            Self::RDI => Some("%rdi"),
            Self::RBP => Some("%rbp"),
            Self::RSP => Some("%rsp"),
            Self::R8 => Some("%r8"),
            Self::R9 => Some("%r9"),
            Self::R10 => Some("%r10"),
            Self::R11 => Some("%r11"),
            Self::R12 => Some("%r12"),
            Self::R13 => Some("%r13"),
            Self::R14 => Some("%r14"),
            Self::R15 => Some("%r15"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reg {
    Phys(PhysReg),
    VReg(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Reg(Reg),
    Imm(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirInst {
    Label(String),
    Mov { dst: Reg, src: Operand },
    Add { dst: Reg, lhs: Reg, rhs: Operand },
    Sub { dst: Reg, lhs: Reg, rhs: Operand },
    And { dst: Reg, lhs: Reg, rhs: Operand },
    Mul { dst: Reg, lhs: Reg, rhs: Operand },
    Sdiv { dst: Reg, lhs: Reg, rhs: Operand },
    Cmp { lhs: Reg, rhs: Operand },
    Push { src: Reg },
    Pop { dst: Reg },
    LoadStack { dst: Reg, offset: i32 },
    StoreStack { src: Reg, offset: i32 },
    Jmp { label: String },
    JmpIf { cond: Cond, label: String },
    Call { symbol: String },
    Ret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFunction {
    pub name: String,
    pub instructions: Vec<MirInst>,
}

impl MirFunction {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            instructions: Vec::new(),
        }
    }

    pub fn with_instructions(name: impl Into<String>, instructions: Vec<MirInst>) -> Self {
        Self {
            name: name.into(),
            instructions,
        }
    }

    pub fn push(&mut self, inst: MirInst) {
        self.instructions.push(inst);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitError {
    message: String,
}

impl EmitError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for EmitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for EmitError {}

pub fn emit_assembly(func: &MirFunction, target: TargetArch) -> Result<String, EmitError> {
    match target {
        TargetArch::Arm64 => emit_arm64_assembly(func),
        TargetArch::Amd64 => emit_x86_64_assembly(func),
        TargetArch::X86_64 => emit_x86_64_assembly(func),
    }
}

pub fn emit_arm64_assembly(func: &MirFunction) -> Result<String, EmitError> {
    let mut out = String::new();
    let func_symbol = mangle_symbol(&func.name);

    out.push_str(".text\n");
    out.push_str(&format!(".globl {}\n", func_symbol));
    out.push_str(".p2align 2\n");
    out.push_str(&format!("{}:\n", func_symbol));

    if let Some(stack_size) = detect_arm64_compact_pair_frame(&func.instructions) {
        if stack_size == 16 {
            out.push_str("    str x30, [sp, #-16]!\n");
        } else {
            out.push_str(&format!("    stp x29, x30, [sp, #-{}]!\n", stack_size));
        }

        for inst in &func.instructions[5..func.instructions.len() - 4] {
            emit_arm64_instruction(inst, &mut out)?;
        }

        if stack_size == 16 {
            out.push_str("    ldr x30, [sp], #16\n");
        } else {
            out.push_str(&format!("    ldp x29, x30, [sp], #{}\n", stack_size));
        }
        out.push_str("    ret\n");
        return Ok(out);
    }

    for inst in &func.instructions {
        emit_arm64_instruction(inst, &mut out)?;
    }

    Ok(out)
}

fn detect_arm64_compact_pair_frame(instructions: &[MirInst]) -> Option<i64> {
    if instructions.len() < 9 {
        return None;
    }

    let stack_size = match &instructions[0..5] {
        [
            MirInst::Mov {
                dst: Reg::Phys(PhysReg::X16),
                src: Operand::Reg(Reg::Phys(PhysReg::X29)),
            },
            MirInst::Sub {
                dst: Reg::Phys(PhysReg::SP),
                lhs: Reg::Phys(PhysReg::SP),
                rhs: Operand::Imm(stack_sub),
            },
            MirInst::Add {
                dst: Reg::Phys(PhysReg::X29),
                lhs: Reg::Phys(PhysReg::SP),
                rhs: Operand::Imm(stack_add),
            },
            MirInst::StoreStack {
                src: Reg::Phys(PhysReg::X16),
                offset: saved_fp_offset,
            },
            MirInst::StoreStack {
                src: Reg::Phys(PhysReg::X30),
                offset: saved_lr_offset,
            },
        ] => {
            if stack_sub != stack_add {
                return None;
            }
            if *stack_sub <= 0 {
                return None;
            }
            if *saved_fp_offset + 8 != *saved_lr_offset {
                return None;
            }
            if i64::from(*saved_lr_offset) != *stack_sub {
                return None;
            }
            *stack_sub
        }
        _ => return None,
    };

    let epilogue_start = instructions.len() - 4;
    match &instructions[epilogue_start..] {
        [
            MirInst::LoadStack {
                dst: Reg::Phys(PhysReg::X30),
                offset: saved_lr_offset,
            },
            MirInst::LoadStack {
                dst: Reg::Phys(PhysReg::X29),
                offset: saved_fp_offset,
            },
            MirInst::Add {
                dst: Reg::Phys(PhysReg::SP),
                lhs: Reg::Phys(PhysReg::SP),
                rhs: Operand::Imm(epilogue_stack),
            },
            MirInst::Ret,
        ] => {
            if *saved_fp_offset + 8 != *saved_lr_offset {
                return None;
            }
            if i64::from(*saved_lr_offset) != stack_size {
                return None;
            }
            if *epilogue_stack != stack_size {
                return None;
            }
        }
        _ => return None,
    }

    // Only compact this frame shape when there are no interior stack accesses.
    if instructions[5..epilogue_start]
        .iter()
        .any(|inst| matches!(inst, MirInst::LoadStack { .. } | MirInst::StoreStack { .. }))
    {
        return None;
    }

    Some(stack_size)
}

fn emit_arm64_instruction(inst: &MirInst, out: &mut String) -> Result<(), EmitError> {
    match inst {
        MirInst::Label(label) => {
            out.push_str(&format!("{}:\n", label));
        }
        MirInst::Mov { dst, src } => {
            let dst = arm64_reg(*dst)?;
            let src = arm64_operand(src)?;
            out.push_str(&format!("    mov {}, {}\n", dst, src));
        }
        MirInst::Add { dst, lhs, rhs } => {
            let dst = arm64_reg(*dst)?;
            let lhs = arm64_reg(*lhs)?;
            let rhs = arm64_operand(rhs)?;
            out.push_str(&format!("    add {}, {}, {}\n", dst, lhs, rhs));
        }
        MirInst::Sub { dst, lhs, rhs } => {
            let dst = arm64_reg(*dst)?;
            let lhs = arm64_reg(*lhs)?;
            let rhs = arm64_operand(rhs)?;
            out.push_str(&format!("    sub {}, {}, {}\n", dst, lhs, rhs));
        }
        MirInst::And { dst, lhs, rhs } => {
            let dst = arm64_reg(*dst)?;
            let lhs = arm64_reg(*lhs)?;
            let rhs = match rhs {
                Operand::Reg(reg) => arm64_reg(*reg)?,
                Operand::Imm(_) => {
                    return Err(EmitError::new("arm64 and requires a register rhs operand"));
                }
            };
            out.push_str(&format!("    and {}, {}, {}\n", dst, lhs, rhs));
        }
        MirInst::Mul { dst, lhs, rhs } => {
            let dst = arm64_reg(*dst)?;
            let lhs = arm64_reg(*lhs)?;
            let rhs = match rhs {
                Operand::Reg(reg) => arm64_reg(*reg)?,
                Operand::Imm(_) => {
                    return Err(EmitError::new("arm64 mul requires a register rhs operand"));
                }
            };
            out.push_str(&format!("    mul {}, {}, {}\n", dst, lhs, rhs));
        }
        MirInst::Sdiv { dst, lhs, rhs } => {
            let dst = arm64_reg(*dst)?;
            let lhs = arm64_reg(*lhs)?;
            let rhs = match rhs {
                Operand::Reg(reg) => arm64_reg(*reg)?,
                Operand::Imm(_) => {
                    return Err(EmitError::new("arm64 sdiv requires a register rhs operand"));
                }
            };
            out.push_str(&format!("    sdiv {}, {}, {}\n", dst, lhs, rhs));
        }
        MirInst::Cmp { lhs, rhs } => {
            let lhs = arm64_reg(*lhs)?;
            let rhs = arm64_operand(rhs)?;
            out.push_str(&format!("    cmp {}, {}\n", lhs, rhs));
        }
        MirInst::Push { .. } | MirInst::Pop { .. } => {
            return Err(EmitError::new(
                "push/pop pseudo-instructions are not supported on arm64",
            ));
        }
        MirInst::LoadStack { dst, offset } => {
            let dst = arm64_reg(*dst)?;
            out.push_str(&format!("    ldr {}, [x29, #-{}]\n", dst, offset));
        }
        MirInst::StoreStack { src, offset } => {
            let src = arm64_reg(*src)?;
            out.push_str(&format!("    str {}, [x29, #-{}]\n", src, offset));
        }
        MirInst::Jmp { label } => {
            out.push_str(&format!("    b {}\n", label));
        }
        MirInst::JmpIf { cond, label } => {
            out.push_str(&format!("    {} {}\n", arm64_cond(*cond), label));
        }
        MirInst::Call { symbol } => {
            out.push_str(&format!("    bl {}\n", mangle_symbol(symbol)));
        }
        MirInst::Ret => {
            out.push_str("    ret\n");
        }
    }

    Ok(())
}

pub fn emit_x86_64_assembly(func: &MirFunction) -> Result<String, EmitError> {
    let mut out = String::new();
    let func_symbol = mangle_symbol(&func.name);

    out.push_str(".text\n");
    out.push_str(&format!(".globl {}\n", func_symbol));
    out.push_str(&format!("{}:\n", func_symbol));

    for inst in &func.instructions {
        match inst {
            MirInst::Label(label) => {
                out.push_str(&format!("{}:\n", label));
            }
            MirInst::Mov { dst, src } => {
                let dst = x86_64_reg(*dst)?;
                match src {
                    Operand::Reg(reg) => {
                        let src = x86_64_reg(*reg)?;
                        out.push_str(&format!("    movq {}, {}\n", src, dst));
                    }
                    Operand::Imm(imm) => {
                        out.push_str(&format!("    movq ${}, {}\n", imm, dst));
                    }
                }
            }
            MirInst::Add { dst, lhs, rhs } => {
                let dst_name = x86_64_reg(*dst)?;
                let lhs_name = x86_64_reg(*lhs)?;

                if dst_name != lhs_name {
                    out.push_str(&format!("    movq {}, {}\n", lhs_name, dst_name));
                }

                match rhs {
                    Operand::Reg(reg) => {
                        let rhs = x86_64_reg(*reg)?;
                        out.push_str(&format!("    addq {}, {}\n", rhs, dst_name));
                    }
                    Operand::Imm(imm) => {
                        out.push_str(&format!("    addq ${}, {}\n", imm, dst_name));
                    }
                }
            }
            MirInst::Sub { dst, lhs, rhs } => {
                let dst_name = x86_64_reg(*dst)?;
                let lhs_name = x86_64_reg(*lhs)?;

                if dst_name != lhs_name {
                    out.push_str(&format!("    movq {}, {}\n", lhs_name, dst_name));
                }

                match rhs {
                    Operand::Reg(reg) => {
                        let rhs = x86_64_reg(*reg)?;
                        out.push_str(&format!("    subq {}, {}\n", rhs, dst_name));
                    }
                    Operand::Imm(imm) => {
                        out.push_str(&format!("    subq ${}, {}\n", imm, dst_name));
                    }
                }
            }
            MirInst::And { dst, lhs, rhs } => {
                let dst_name = x86_64_reg(*dst)?;
                let lhs_name = x86_64_reg(*lhs)?;

                if dst_name != lhs_name {
                    out.push_str(&format!("    movq {}, {}\n", lhs_name, dst_name));
                }

                match rhs {
                    Operand::Reg(reg) => {
                        let rhs = x86_64_reg(*reg)?;
                        out.push_str(&format!("    andq {}, {}\n", rhs, dst_name));
                    }
                    Operand::Imm(imm) => {
                        out.push_str(&format!("    andq ${}, {}\n", imm, dst_name));
                    }
                }
            }
            MirInst::Mul { dst, lhs, rhs } => {
                let dst_name = x86_64_reg(*dst)?;
                let lhs_name = x86_64_reg(*lhs)?;

                if dst_name != lhs_name {
                    out.push_str(&format!("    movq {}, {}\n", lhs_name, dst_name));
                }

                match rhs {
                    Operand::Reg(reg) => {
                        let rhs = x86_64_reg(*reg)?;
                        out.push_str(&format!("    imulq {}, {}\n", rhs, dst_name));
                    }
                    Operand::Imm(imm) => {
                        out.push_str(&format!("    imulq ${}, {}\n", imm, dst_name));
                    }
                }
            }
            MirInst::Sdiv { dst, lhs, rhs } => {
                let dst_name = x86_64_reg(*dst)?;
                let lhs_name = x86_64_reg(*lhs)?;

                let mut rhs_override = None;
                if let Operand::Reg(reg) = rhs {
                    let rhs_name = x86_64_reg(*reg)?;
                    if rhs_name == "%rax" && lhs_name != "%rax" {
                        out.push_str("    movq %rax, %r11\n");
                        rhs_override = Some("%r11");
                    }
                }

                if lhs_name != "%rax" {
                    out.push_str(&format!("    movq {}, %rax\n", lhs_name));
                }

                out.push_str("    cqto\n");

                match rhs {
                    Operand::Reg(reg) => {
                        if let Some(rhs) = rhs_override {
                            out.push_str(&format!("    idivq {}\n", rhs));
                        } else {
                            let rhs = x86_64_reg(*reg)?;
                            out.push_str(&format!("    idivq {}\n", rhs));
                        }
                    }
                    Operand::Imm(imm) => {
                        out.push_str(&format!("    movq ${}, %r11\n", imm));
                        out.push_str("    idivq %r11\n");
                    }
                }

                if dst_name != "%rax" {
                    out.push_str(&format!("    movq %rax, {}\n", dst_name));
                }
            }
            MirInst::Cmp { lhs, rhs } => {
                let lhs = x86_64_reg(*lhs)?;
                match rhs {
                    Operand::Reg(reg) => {
                        let rhs = x86_64_reg(*reg)?;
                        out.push_str(&format!("    cmpq {}, {}\n", rhs, lhs));
                    }
                    Operand::Imm(imm) => {
                        out.push_str(&format!("    cmpq ${}, {}\n", imm, lhs));
                    }
                }
            }
            MirInst::Push { src } => {
                let src = x86_64_reg(*src)?;
                out.push_str(&format!("    pushq {}\n", src));
            }
            MirInst::Pop { dst } => {
                let dst = x86_64_reg(*dst)?;
                out.push_str(&format!("    popq {}\n", dst));
            }
            MirInst::LoadStack { dst, offset } => {
                let dst = x86_64_reg(*dst)?;
                out.push_str(&format!(
                    "    movq {}, {}\n",
                    x86_64_stack_addr(*offset),
                    dst
                ));
            }
            MirInst::StoreStack { src, offset } => {
                let src = x86_64_reg(*src)?;
                out.push_str(&format!(
                    "    movq {}, {}\n",
                    src,
                    x86_64_stack_addr(*offset)
                ));
            }
            MirInst::Jmp { label } => {
                out.push_str(&format!("    jmp {}\n", label));
            }
            MirInst::JmpIf { cond, label } => {
                out.push_str(&format!("    {} {}\n", x86_64_cond(*cond), label));
            }
            MirInst::Call { symbol } => {
                out.push_str(&format!("    call {}\n", mangle_symbol(symbol)));
            }
            MirInst::Ret => {
                out.push_str("    ret\n");
            }
        }
    }

    Ok(out)
}

fn arm64_reg(reg: Reg) -> Result<String, EmitError> {
    match reg {
        Reg::Phys(phys) => phys
            .as_arm64_name()
            .map(ToOwned::to_owned)
            .ok_or_else(|| EmitError::new(format!("register {phys:?} is not valid on arm64"))),
        Reg::VReg(id) => Err(EmitError::new(format!(
            "cannot emit virtual register v{id} without register allocation"
        ))),
    }
}

fn x86_64_reg(reg: Reg) -> Result<String, EmitError> {
    match reg {
        Reg::Phys(phys) => phys
            .as_x86_64_name()
            .map(ToOwned::to_owned)
            .ok_or_else(|| EmitError::new(format!("register {phys:?} is not valid on x86_64"))),
        Reg::VReg(id) => Err(EmitError::new(format!(
            "cannot emit virtual register v{id} without register allocation"
        ))),
    }
}

fn arm64_operand(op: &Operand) -> Result<String, EmitError> {
    match op {
        Operand::Reg(reg) => arm64_reg(*reg),
        Operand::Imm(imm) => Ok(format!("#{imm}")),
    }
}

fn arm64_cond(cond: Cond) -> &'static str {
    match cond {
        Cond::Eq => "b.eq",
        Cond::Ne => "b.ne",
        Cond::Lt => "b.lt",
        Cond::Le => "b.le",
        Cond::Gt => "b.gt",
        Cond::Ge => "b.ge",
    }
}

fn x86_64_cond(cond: Cond) -> &'static str {
    match cond {
        Cond::Eq => "je",
        Cond::Ne => "jne",
        Cond::Lt => "jl",
        Cond::Le => "jle",
        Cond::Gt => "jg",
        Cond::Ge => "jge",
    }
}

fn x86_64_stack_addr(offset: i32) -> String {
    if offset > 0 {
        format!("-{}(%rbp)", offset)
    } else if offset < 0 {
        format!("{}(%rbp)", -offset)
    } else {
        "(%rbp)".to_string()
    }
}

fn mangle_symbol(symbol: &str) -> String {
    if cfg!(target_os = "macos") && !symbol.starts_with('_') {
        format!("_{symbol}")
    } else {
        symbol.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_arm64_smoke_function() {
        let func = MirFunction::with_instructions(
            "main",
            vec![
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::X0),
                    src: Operand::Imm(42),
                },
                MirInst::Ret,
            ],
        );

        let asm = emit_arm64_assembly(&func).expect("arm64 emission failed");

        assert!(asm.contains("mov x0, #42"));
        assert!(asm.contains("ret"));
    }

    #[test]
    fn rejects_virtual_register_during_emission() {
        let func = MirFunction::with_instructions(
            "main",
            vec![MirInst::Mov {
                dst: Reg::VReg(0),
                src: Operand::Imm(1),
            }],
        );

        let err = emit_arm64_assembly(&func).expect_err("expected vreg to fail");
        assert!(err.to_string().contains("virtual register"));
    }

    #[test]
    fn amd64_alias_uses_x86_64_emitter() {
        let func = MirFunction::with_instructions(
            "main",
            vec![
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RAX),
                    src: Operand::Imm(42),
                },
                MirInst::Ret,
            ],
        );

        let asm = emit_assembly(&func, TargetArch::Amd64).expect("amd64 emission failed");
        assert!(asm.contains("movq $42, %rax"));
    }
}
