use king_compiler::ir::build_factorial_il;
use king_compiler::mir::{
    MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch, emit_arm64_assembly,
    emit_x86_64_assembly,
};
use king_compiler::regalloc::linear_scan_allocate;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arm64_func = MirFunction::with_instructions(
        "main",
        vec![
            MirInst::Mov {
                dst: Reg::Phys(PhysReg::X0),
                src: Operand::Imm(42),
            },
            MirInst::Ret,
        ],
    );
    let arm64_asm = emit_arm64_assembly(&arm64_func)?;
    std::fs::write("test_arm64.s", &arm64_asm)?;

    let x86_64_func = MirFunction::with_instructions(
        "main",
        vec![
            MirInst::Mov {
                dst: Reg::Phys(PhysReg::RAX),
                src: Operand::Imm(42),
            },
            MirInst::Ret,
        ],
    );
    let x86_64_asm = emit_x86_64_assembly(&x86_64_func)?;
    std::fs::write("test_x86_64.s", &x86_64_asm)?;

    let phase2_input = build_linear_scan_phase2_demo();
    let allocated = linear_scan_allocate(&phase2_input, TargetArch::X86_64)?;
    let x86_64_linear_scan_asm = emit_x86_64_assembly(&allocated.function)?;
    std::fs::write("test_linear_scan_x86_64.s", &x86_64_linear_scan_asm)?;

    if cfg!(target_arch = "aarch64") {
        std::fs::write("test.s", &arm64_asm)?;
        println!("Wrote test.s for host arm64, plus test_arm64.s and test_x86_64.s");
    } else if cfg!(target_arch = "x86_64") {
        std::fs::write("test.s", &x86_64_asm)?;
        println!("Wrote test.s for host x86_64, plus test_arm64.s and test_x86_64.s");
    } else {
        println!("Wrote test_arm64.s and test_x86_64.s");
    }

    println!(
        "Wrote test_linear_scan_x86_64.s (stack frame: {} bytes)",
        allocated.stack_size
    );

    let factorial = build_factorial_il()?;
    let factorial_text = factorial.format_il();
    std::fs::write("factorial.il", &factorial_text)?;
    println!("Phase 3 factorial IL:\n{factorial_text}");

    Ok(())
}

fn build_linear_scan_phase2_demo() -> MirFunction {
    let mut instructions = Vec::new();

    for vreg in 0..30 {
        instructions.push(MirInst::Mov {
            dst: Reg::VReg(vreg),
            src: Operand::Imm(vreg as i64 + 1),
        });
    }

    instructions.push(MirInst::Add {
        dst: Reg::VReg(30),
        lhs: Reg::VReg(28),
        rhs: Operand::Reg(Reg::VReg(29)),
    });
    instructions.push(MirInst::Call {
        symbol: "helper".to_string(),
    });
    instructions.push(MirInst::Add {
        dst: Reg::VReg(31),
        lhs: Reg::VReg(30),
        rhs: Operand::Imm(12),
    });
    instructions.push(MirInst::Mov {
        dst: Reg::Phys(PhysReg::RAX),
        src: Operand::Reg(Reg::VReg(31)),
    });
    instructions.push(MirInst::Ret);

    MirFunction::with_instructions("phase2_linear_scan", instructions)
}
