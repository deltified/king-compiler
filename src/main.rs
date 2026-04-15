use king_compiler::ir::{IrBuilder, Type, build_factorial_il, run_phase5_pipeline};
use king_compiler::lowering::lower_il_to_mir;
use king_compiler::mir::{
    MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch, emit_arm64_assembly, emit_assembly,
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

    let phase4_main_il = build_phase4_main_il()?;
    let phase4_main_mir = lower_il_to_mir(&phase4_main_il, TargetArch::Amd64)?;
    let phase4_main_allocated = linear_scan_allocate(&phase4_main_mir, TargetArch::Amd64)?;
    let phase4_main_asm = emit_assembly(&phase4_main_allocated.function, TargetArch::Amd64)?;
    std::fs::write("phase4_main_x86_64.s", &phase4_main_asm)?;

    let phase4_factorial_mir = lower_il_to_mir(&factorial, TargetArch::Amd64)?;
    let phase4_factorial_allocated =
        linear_scan_allocate(&phase4_factorial_mir, TargetArch::Amd64)?;
    let phase4_factorial_asm =
        emit_assembly(&phase4_factorial_allocated.function, TargetArch::Amd64)?;
    std::fs::write("phase4_factorial_x86_64.s", &phase4_factorial_asm)?;

    let phase4_extended_il = build_phase4_extended_il()?;
    let phase4_extended_mir = lower_il_to_mir(&phase4_extended_il, TargetArch::Amd64)?;
    let phase4_extended_allocated = linear_scan_allocate(&phase4_extended_mir, TargetArch::Amd64)?;
    let phase4_extended_asm =
        emit_assembly(&phase4_extended_allocated.function, TargetArch::Amd64)?;
    std::fs::write("phase4_extended_x86_64.s", &phase4_extended_asm)?;

    let phase4_call_il = build_phase4_call_main_il()?;
    let phase4_call_mir = lower_il_to_mir(&phase4_call_il, TargetArch::Amd64)?;
    let phase4_call_allocated = linear_scan_allocate(&phase4_call_mir, TargetArch::Amd64)?;
    let phase4_call_asm = emit_assembly(&phase4_call_allocated.function, TargetArch::Amd64)?;
    std::fs::write("phase4_call_main_x86_64.s", &phase4_call_asm)?;

    let phase4_identity_il = build_phase4_identity_il()?;
    let phase4_identity_mir = lower_il_to_mir(&phase4_identity_il, TargetArch::Amd64)?;
    let phase4_identity_allocated = linear_scan_allocate(&phase4_identity_mir, TargetArch::Amd64)?;
    let phase4_identity_asm =
        emit_assembly(&phase4_identity_allocated.function, TargetArch::Amd64)?;
    std::fs::write("phase4_identity_x86_64.s", &phase4_identity_asm)?;

    let phase4_add2_il = build_phase4_add2_il()?;
    let phase4_add2_mir = lower_il_to_mir(&phase4_add2_il, TargetArch::Amd64)?;
    let phase4_add2_allocated = linear_scan_allocate(&phase4_add2_mir, TargetArch::Amd64)?;
    let phase4_add2_asm = emit_assembly(&phase4_add2_allocated.function, TargetArch::Amd64)?;
    std::fs::write("phase4_add2_x86_64.s", &phase4_add2_asm)?;

    println!(
        "Phase 4 wrote phase4_main_x86_64.s, phase4_factorial_x86_64.s, phase4_extended_x86_64.s, phase4_call_main_x86_64.s, phase4_identity_x86_64.s, and phase4_add2_x86_64.s"
    );

    let phase5_function = run_phase5_pipeline(factorial.clone());
    let phase5_text = phase5_function.format_il();
    std::fs::write("phase5_factorial.il", &phase5_text)?;
    let phase5_mir = lower_il_to_mir(&phase5_function, TargetArch::Amd64)?;
    let phase5_allocated = linear_scan_allocate(&phase5_mir, TargetArch::Amd64)?;
    let phase5_asm = emit_assembly(&phase5_allocated.function, TargetArch::Amd64)?;
    std::fs::write("phase5_factorial_x86_64.s", &phase5_asm)?;

    println!("Phase 5 wrote phase5_factorial.il and phase5_factorial_x86_64.s");

    Ok(())
}

fn build_phase4_main_il() -> Result<king_compiler::ir::Function, king_compiler::ir::IrBuildError> {
    let mut builder = IrBuilder::new("main", Type::I32);
    let entry = builder.create_block("entry");
    builder.position_at_end(entry)?;
    let value_42 = builder.build_const_i32(42)?;
    builder.build_ret(Some(value_42))?;
    Ok(builder.finish())
}

fn build_phase4_extended_il() -> Result<king_compiler::ir::Function, king_compiler::ir::IrBuildError>
{
    let mut builder = IrBuilder::new("main", Type::I32);
    let entry = builder.create_block("entry");
    builder.position_at_end(entry)?;

    let ptr = builder.build_alloca(Type::I32)?;
    let eighty_four = builder.build_const_i32(84)?;
    builder.build_store(Type::I32, eighty_four, ptr)?;
    let loaded = builder.build_load(Type::I32, ptr)?;
    let two = builder.build_const_i32(2)?;
    let divided = builder.build_sdiv(loaded, two)?;
    let mask = builder.build_const_i32(31)?;
    let masked = builder.build_and(divided, mask)?;
    builder.build_ret(Some(masked))?;

    Ok(builder.finish())
}

fn build_phase4_call_main_il()
-> Result<king_compiler::ir::Function, king_compiler::ir::IrBuildError> {
    let mut builder = IrBuilder::new("main", Type::I32);
    let entry = builder.create_block("entry");
    builder.position_at_end(entry)?;

    let four = builder.build_const_i32(4)?;
    let two = builder.build_const_i32(2)?;
    let packed = builder
        .build_call(
            Type::I32,
            "pack2",
            vec![(Type::I32, four), (Type::I32, two)],
        )?
        .expect("pack2 call should return a value");
    builder.build_ret(Some(packed))?;

    Ok(builder.finish())
}

fn build_phase4_identity_il() -> Result<king_compiler::ir::Function, king_compiler::ir::IrBuildError>
{
    let mut builder = IrBuilder::new("identity", Type::I32);
    let n = builder.add_param("n", Type::I32);
    let entry = builder.create_block("entry");
    builder.position_at_end(entry)?;
    builder.build_ret(Some(n))?;
    Ok(builder.finish())
}

fn build_phase4_add2_il() -> Result<king_compiler::ir::Function, king_compiler::ir::IrBuildError> {
    let mut builder = IrBuilder::new("add2", Type::I32);
    let a = builder.add_param("a", Type::I32);
    let b = builder.add_param("b", Type::I32);
    let entry = builder.create_block("entry");
    builder.position_at_end(entry)?;
    let sum = builder.build_add(a, b)?;
    builder.build_ret(Some(sum))?;
    Ok(builder.finish())
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
