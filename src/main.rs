use king_compiler::mir::{
    MirFunction, MirInst, Operand, PhysReg, Reg, emit_arm64_assembly, emit_x86_64_assembly,
};

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

    if cfg!(target_arch = "aarch64") {
        std::fs::write("test.s", &arm64_asm)?;
        println!("Wrote test.s for host arm64, plus test_arm64.s and test_x86_64.s");
    } else if cfg!(target_arch = "x86_64") {
        std::fs::write("test.s", &x86_64_asm)?;
        println!("Wrote test.s for host x86_64, plus test_arm64.s and test_x86_64.s");
    } else {
        println!("Wrote test_arm64.s and test_x86_64.s");
    }

    Ok(())
}
