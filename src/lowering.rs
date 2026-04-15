use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::ir::{BlockId, Function, IcmpPredicate, InstrId, Instruction, Type, ValueId, ValueKind};
use crate::mir::{Cond, MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhiCopy {
    pub dst: ValueId,
    pub src: ValueId,
}

#[derive(Debug, Clone, Default)]
pub struct PhiElimination {
    pub edge_copies: HashMap<(BlockId, BlockId), Vec<PhiCopy>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweringError {
    UnsupportedTarget(TargetArch),
    UnsupportedInstruction(&'static str),
    UnsupportedPointerValue(ValueId),
    UnknownBlock(BlockId),
    UnknownValue(ValueId),
    MissingInstructionResult(InstrId),
    TooManyParams(usize),
    TooManyCallArgs(usize),
}

impl Display for LoweringError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedTarget(target) => {
                write!(
                    f,
                    "target {target:?} is not yet supported by the IL->MIR lowerer"
                )
            }
            Self::UnsupportedInstruction(name) => {
                write!(f, "instruction {name} is not yet supported by the lowerer")
            }
            Self::UnsupportedPointerValue(value) => {
                write!(
                    f,
                    "pointer value {value:?} is not from alloca and cannot be lowered yet"
                )
            }
            Self::UnknownBlock(block) => write!(f, "unknown block: {block:?}"),
            Self::UnknownValue(value) => write!(f, "unknown value: {value:?}"),
            Self::MissingInstructionResult(instr) => {
                write!(f, "missing result value for instruction: {instr:?}")
            }
            Self::TooManyParams(count) => {
                write!(
                    f,
                    "function has {count} parameters, but only up to 6 are currently supported"
                )
            }
            Self::TooManyCallArgs(count) => {
                write!(
                    f,
                    "call has {count} arguments, but only up to 6 are currently supported"
                )
            }
        }
    }
}

impl Error for LoweringError {}

pub fn eliminate_phi_nodes(function: &Function) -> Result<PhiElimination, LoweringError> {
    let instr_results = instruction_result_map(function);
    let mut edge_copies: HashMap<(BlockId, BlockId), Vec<PhiCopy>> = HashMap::new();

    for (block_id, block) in &function.blocks {
        for instr_id in &block.instructions {
            let Some(instruction) = function.instruction(*instr_id) else {
                continue;
            };

            let Instruction::Phi { incomings, .. } = instruction else {
                break;
            };

            let dst = instr_results
                .get(instr_id)
                .copied()
                .ok_or(LoweringError::MissingInstructionResult(*instr_id))?;

            for incoming in incomings {
                if !function.blocks.contains_key(incoming.block) {
                    return Err(LoweringError::UnknownBlock(incoming.block));
                }

                edge_copies
                    .entry((incoming.block, block_id))
                    .or_default()
                    .push(PhiCopy {
                        dst,
                        src: incoming.value,
                    });
            }
        }
    }

    Ok(PhiElimination { edge_copies })
}

pub fn lower_il_to_mir(
    function: &Function,
    target: TargetArch,
) -> Result<MirFunction, LoweringError> {
    match target {
        TargetArch::Amd64 | TargetArch::X86_64 => {}
        TargetArch::Arm64 => return Err(LoweringError::UnsupportedTarget(target)),
    }

    let phi = eliminate_phi_nodes(function)?;
    let instr_results = instruction_result_map(function);

    let mut block_entries: Vec<(BlockId, String, Vec<InstrId>)> = Vec::new();
    for (block_id, block) in &function.blocks {
        block_entries.push((block_id, block.name.clone(), block.instructions.clone()));
    }

    let mut block_indices = HashMap::new();
    for (index, (block_id, _, _)) in block_entries.iter().enumerate() {
        block_indices.insert(*block_id, index);
    }

    let mut block_labels = HashMap::new();
    for (index, (block_id, name, _)) in block_entries.iter().enumerate() {
        block_labels.insert(*block_id, format!("bb{index}_{}", sanitize_label(name)));
    }

    let mut edge_labels = HashMap::new();
    for (from, to) in phi.edge_copies.keys() {
        let from_index = block_indices
            .get(from)
            .copied()
            .ok_or(LoweringError::UnknownBlock(*from))?;
        let to_index = block_indices
            .get(to)
            .copied()
            .ok_or(LoweringError::UnknownBlock(*to))?;
        edge_labels.insert((*from, *to), format!("edge_{from_index}_{to_index}"));
    }

    let mut value_vregs = HashMap::new();
    let mut next_vreg = 0usize;
    let mut alloca_slots: HashMap<ValueId, i32> = HashMap::new();
    let mut next_alloca_offset: i32 = 0;
    let arg_regs = [
        PhysReg::RDI,
        PhysReg::RSI,
        PhysReg::RDX,
        PhysReg::RCX,
        PhysReg::R8,
        PhysReg::R9,
    ];
    for (value_id, value) in &function.values {
        if !matches!(value.kind, ValueKind::ConstantInt(_)) {
            value_vregs.insert(value_id, next_vreg);
            next_vreg += 1;
        }
    }

    if function.params.len() > arg_regs.len() {
        return Err(LoweringError::TooManyParams(function.params.len()));
    }

    let mut out = Vec::new();
    let mut icmp_label_counter = 0usize;
    let entry_block = block_entries.first().map(|(block_id, _, _)| *block_id);

    for (block_id, _, instructions) in &block_entries {
        let label = block_labels
            .get(block_id)
            .cloned()
            .ok_or(LoweringError::UnknownBlock(*block_id))?;
        out.push(MirInst::Label(label));

        if Some(*block_id) == entry_block {
            for (index, param) in function.params.iter().enumerate() {
                let dst = value_reg(param.value, &value_vregs)?;
                out.push(MirInst::Mov {
                    dst,
                    src: Operand::Reg(Reg::Phys(arg_regs[index])),
                });
            }
        }

        for instr_id in instructions {
            let instruction = function
                .instruction(*instr_id)
                .ok_or(LoweringError::MissingInstructionResult(*instr_id))?;

            match instruction {
                Instruction::Phi { .. } => {
                    // Phi nodes are removed by emitting edge copies in dedicated edge labels.
                }
                Instruction::Add { lhs, rhs, .. } => {
                    lower_integer_binop(
                        function,
                        &instr_results,
                        &value_vregs,
                        instr_id,
                        *lhs,
                        *rhs,
                        &mut next_vreg,
                        &mut out,
                        IntBinOp::Add,
                    )?;
                }
                Instruction::Sub { lhs, rhs, .. } => {
                    lower_integer_binop(
                        function,
                        &instr_results,
                        &value_vregs,
                        instr_id,
                        *lhs,
                        *rhs,
                        &mut next_vreg,
                        &mut out,
                        IntBinOp::Sub,
                    )?;
                }
                Instruction::And { lhs, rhs, .. } => {
                    lower_integer_binop(
                        function,
                        &instr_results,
                        &value_vregs,
                        instr_id,
                        *lhs,
                        *rhs,
                        &mut next_vreg,
                        &mut out,
                        IntBinOp::And,
                    )?;
                }
                Instruction::Mul { lhs, rhs, .. } => {
                    lower_integer_binop(
                        function,
                        &instr_results,
                        &value_vregs,
                        instr_id,
                        *lhs,
                        *rhs,
                        &mut next_vreg,
                        &mut out,
                        IntBinOp::Mul,
                    )?;
                }
                Instruction::Sdiv { lhs, rhs, .. } => {
                    lower_integer_binop(
                        function,
                        &instr_results,
                        &value_vregs,
                        instr_id,
                        *lhs,
                        *rhs,
                        &mut next_vreg,
                        &mut out,
                        IntBinOp::Sdiv,
                    )?;
                }
                Instruction::Icmp { pred, lhs, rhs, .. } => {
                    let dst_value = instr_results
                        .get(instr_id)
                        .copied()
                        .ok_or(LoweringError::MissingInstructionResult(*instr_id))?;
                    let dst = value_reg(dst_value, &value_vregs)?;

                    emit_cmp_values(function, *lhs, *rhs, &value_vregs, &mut next_vreg, &mut out)?;

                    let true_label = format!("icmp_true_{icmp_label_counter}");
                    let done_label = format!("icmp_done_{icmp_label_counter}");
                    icmp_label_counter += 1;

                    out.push(MirInst::Mov {
                        dst,
                        src: Operand::Imm(0),
                    });
                    out.push(MirInst::JmpIf {
                        cond: cond_from_icmp(*pred),
                        label: true_label.clone(),
                    });
                    out.push(MirInst::Jmp {
                        label: done_label.clone(),
                    });
                    out.push(MirInst::Label(true_label));
                    out.push(MirInst::Mov {
                        dst,
                        src: Operand::Imm(1),
                    });
                    out.push(MirInst::Label(done_label));
                }
                Instruction::Br {
                    cond,
                    then_block,
                    else_block,
                } => {
                    let cond_operand = value_operand(function, *cond, &value_vregs)?;
                    let cond_reg = ensure_operand_reg(cond_operand, &mut next_vreg, &mut out);
                    out.push(MirInst::Cmp {
                        lhs: cond_reg,
                        rhs: Operand::Imm(0),
                    });

                    let then_label =
                        edge_or_block_label(*block_id, *then_block, &edge_labels, &block_labels)?;
                    let else_label =
                        edge_or_block_label(*block_id, *else_block, &edge_labels, &block_labels)?;

                    out.push(MirInst::JmpIf {
                        cond: Cond::Ne,
                        label: then_label,
                    });
                    out.push(MirInst::Jmp { label: else_label });
                }
                Instruction::Jmp { target } => {
                    let target_label =
                        edge_or_block_label(*block_id, *target, &edge_labels, &block_labels)?;
                    out.push(MirInst::Jmp {
                        label: target_label,
                    });
                }
                Instruction::Ret { value } => {
                    if let Some(value) = value {
                        out.push(MirInst::Mov {
                            dst: Reg::Phys(PhysReg::RAX),
                            src: value_operand(function, *value, &value_vregs)?,
                        });
                    }
                    out.push(MirInst::Ret);
                }
                Instruction::Call {
                    ret_ty,
                    function: callee,
                    args,
                } => {
                    if args.len() > arg_regs.len() {
                        return Err(LoweringError::TooManyCallArgs(args.len()));
                    }

                    let mut staged_offsets = Vec::with_capacity(args.len());
                    for (_, arg_value) in args {
                        let operand = value_operand(function, *arg_value, &value_vregs)?;
                        let arg_reg = ensure_operand_reg(operand, &mut next_vreg, &mut out);
                        next_alloca_offset += 8;
                        let offset = next_alloca_offset;
                        staged_offsets.push(offset);
                        out.push(MirInst::StoreStack {
                            src: arg_reg,
                            offset,
                        });
                    }

                    for (index, offset) in staged_offsets.into_iter().enumerate() {
                        out.push(MirInst::LoadStack {
                            dst: Reg::Phys(arg_regs[index]),
                            offset,
                        });
                    }

                    out.push(MirInst::Call {
                        symbol: callee.clone(),
                    });

                    if *ret_ty != Type::Void {
                        let dst_value = instr_results
                            .get(instr_id)
                            .copied()
                            .ok_or(LoweringError::MissingInstructionResult(*instr_id))?;
                        let dst = value_reg(dst_value, &value_vregs)?;
                        out.push(MirInst::Mov {
                            dst,
                            src: Operand::Reg(Reg::Phys(PhysReg::RAX)),
                        });
                    }
                }
                Instruction::Alloca { ty } => {
                    let dst_value = instr_results
                        .get(instr_id)
                        .copied()
                        .ok_or(LoweringError::MissingInstructionResult(*instr_id))?;
                    next_alloca_offset += alloca_slot_size(*ty) as i32;
                    alloca_slots.insert(dst_value, next_alloca_offset);
                }
                Instruction::Store { value, ptr, .. } => {
                    let offset = resolve_alloca_offset(*ptr, &alloca_slots)?;
                    let src_operand = value_operand(function, *value, &value_vregs)?;
                    let src_reg = ensure_operand_reg(src_operand, &mut next_vreg, &mut out);
                    out.push(MirInst::StoreStack {
                        src: src_reg,
                        offset,
                    });
                }
                Instruction::Load { ptr, .. } => {
                    let dst_value = instr_results
                        .get(instr_id)
                        .copied()
                        .ok_or(LoweringError::MissingInstructionResult(*instr_id))?;
                    let dst = value_reg(dst_value, &value_vregs)?;
                    let offset = resolve_alloca_offset(*ptr, &alloca_slots)?;
                    out.push(MirInst::LoadStack { dst, offset });
                }
            }
        }
    }

    let mut edge_entries: Vec<_> = phi.edge_copies.iter().collect();
    edge_entries.sort_by_key(|((from, to), _)| {
        (
            block_indices.get(from).copied().unwrap_or(usize::MAX),
            block_indices.get(to).copied().unwrap_or(usize::MAX),
        )
    });

    for ((from, to), copies) in edge_entries {
        let edge_label = edge_labels
            .get(&(*from, *to))
            .cloned()
            .ok_or(LoweringError::UnknownBlock(*from))?;
        let target_label = block_labels
            .get(to)
            .cloned()
            .ok_or(LoweringError::UnknownBlock(*to))?;

        out.push(MirInst::Label(edge_label));
        for copy in copies {
            let dst = value_reg(copy.dst, &value_vregs)?;
            let src = value_operand(function, copy.src, &value_vregs)?;
            out.push(MirInst::Mov { dst, src });
        }
        out.push(MirInst::Jmp {
            label: target_label,
        });
    }

    Ok(MirFunction::with_instructions(function.name.clone(), out))
}

fn instruction_result_map(function: &Function) -> HashMap<InstrId, ValueId> {
    let mut map = HashMap::new();
    for (value_id, data) in &function.values {
        if let ValueKind::InstructionResult(instr_id) = data.kind {
            map.insert(instr_id, value_id);
        }
    }
    map
}

fn value_operand(
    function: &Function,
    value: ValueId,
    value_vregs: &HashMap<ValueId, usize>,
) -> Result<Operand, LoweringError> {
    let data = function
        .value(value)
        .ok_or(LoweringError::UnknownValue(value))?;
    match data.kind {
        ValueKind::ConstantInt(number) => Ok(Operand::Imm(number)),
        ValueKind::Parameter(_) | ValueKind::InstructionResult(_) => {
            let vreg = value_vregs
                .get(&value)
                .copied()
                .ok_or(LoweringError::UnknownValue(value))?;
            Ok(Operand::Reg(Reg::VReg(vreg)))
        }
    }
}

fn value_reg(value: ValueId, value_vregs: &HashMap<ValueId, usize>) -> Result<Reg, LoweringError> {
    value_vregs
        .get(&value)
        .copied()
        .map(Reg::VReg)
        .ok_or(LoweringError::UnknownValue(value))
}

fn ensure_operand_reg(operand: Operand, next_vreg: &mut usize, out: &mut Vec<MirInst>) -> Reg {
    match operand {
        Operand::Reg(reg) => reg,
        Operand::Imm(imm) => {
            let reg = Reg::VReg(*next_vreg);
            *next_vreg += 1;
            out.push(MirInst::Mov {
                dst: reg,
                src: Operand::Imm(imm),
            });
            reg
        }
    }
}

fn emit_cmp_values(
    function: &Function,
    lhs: ValueId,
    rhs: ValueId,
    value_vregs: &HashMap<ValueId, usize>,
    next_vreg: &mut usize,
    out: &mut Vec<MirInst>,
) -> Result<(), LoweringError> {
    let lhs_operand = value_operand(function, lhs, value_vregs)?;
    let rhs_operand = value_operand(function, rhs, value_vregs)?;
    let lhs_reg = ensure_operand_reg(lhs_operand, next_vreg, out);
    out.push(MirInst::Cmp {
        lhs: lhs_reg,
        rhs: rhs_operand,
    });
    Ok(())
}

fn edge_or_block_label(
    from: BlockId,
    to: BlockId,
    edge_labels: &HashMap<(BlockId, BlockId), String>,
    block_labels: &HashMap<BlockId, String>,
) -> Result<String, LoweringError> {
    if let Some(label) = edge_labels.get(&(from, to)) {
        Ok(label.clone())
    } else {
        block_labels
            .get(&to)
            .cloned()
            .ok_or(LoweringError::UnknownBlock(to))
    }
}

fn cond_from_icmp(pred: IcmpPredicate) -> Cond {
    match pred {
        IcmpPredicate::Eq => Cond::Eq,
        IcmpPredicate::Ne => Cond::Ne,
        IcmpPredicate::Slt => Cond::Lt,
        IcmpPredicate::Sle => Cond::Le,
        IcmpPredicate::Sgt => Cond::Gt,
        IcmpPredicate::Sge => Cond::Ge,
    }
}

fn alloca_slot_size(ty: Type) -> usize {
    match ty {
        Type::I8 => 8,
        Type::I32 => 8,
        Type::I64 => 8,
        Type::Ptr => 8,
        Type::Void => 8,
    }
}

fn resolve_alloca_offset(
    ptr: ValueId,
    alloca_slots: &HashMap<ValueId, i32>,
) -> Result<i32, LoweringError> {
    alloca_slots
        .get(&ptr)
        .copied()
        .ok_or(LoweringError::UnsupportedPointerValue(ptr))
}

fn sanitize_label(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "block".to_string()
    } else {
        out
    }
}

#[derive(Debug, Clone, Copy)]
enum IntBinOp {
    Add,
    Sub,
    And,
    Mul,
    Sdiv,
}

#[allow(clippy::too_many_arguments)]
fn lower_integer_binop(
    function: &Function,
    instr_results: &HashMap<InstrId, ValueId>,
    value_vregs: &HashMap<ValueId, usize>,
    instr_id: &InstrId,
    lhs: ValueId,
    rhs: ValueId,
    next_vreg: &mut usize,
    out: &mut Vec<MirInst>,
    op: IntBinOp,
) -> Result<(), LoweringError> {
    let dst_value = instr_results
        .get(instr_id)
        .copied()
        .ok_or(LoweringError::MissingInstructionResult(*instr_id))?;
    let dst = value_reg(dst_value, value_vregs)?;
    let lhs_operand = value_operand(function, lhs, value_vregs)?;
    let rhs_operand = value_operand(function, rhs, value_vregs)?;

    let lhs_reg = ensure_operand_reg(lhs_operand, next_vreg, out);
    if lhs_reg != dst {
        out.push(MirInst::Mov {
            dst,
            src: Operand::Reg(lhs_reg),
        });
    }

    match op {
        IntBinOp::Add => out.push(MirInst::Add {
            dst,
            lhs: dst,
            rhs: rhs_operand,
        }),
        IntBinOp::Sub => out.push(MirInst::Sub {
            dst,
            lhs: dst,
            rhs: rhs_operand,
        }),
        IntBinOp::And => out.push(MirInst::And {
            dst,
            lhs: dst,
            rhs: rhs_operand,
        }),
        IntBinOp::Mul => out.push(MirInst::Mul {
            dst,
            lhs: dst,
            rhs: rhs_operand,
        }),
        IntBinOp::Sdiv => out.push(MirInst::Sdiv {
            dst,
            lhs: dst,
            rhs: rhs_operand,
        }),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrBuilder, Type, build_factorial_il};
    use crate::mir::emit_x86_64_assembly;
    use crate::regalloc::linear_scan_allocate;

    #[test]
    fn phi_elimination_collects_factorial_edges() {
        let function = build_factorial_il().expect("factorial IL should build");
        let phi = eliminate_phi_nodes(&function).expect("phi elimination should succeed");

        assert!(!phi.edge_copies.is_empty());
        let total_copies: usize = phi.edge_copies.values().map(|copies| copies.len()).sum();
        assert_eq!(total_copies, 6);
    }

    #[test]
    fn lowers_factorial_with_edge_copy_labels() {
        let function = build_factorial_il().expect("factorial IL should build");
        let mir = lower_il_to_mir(&function, TargetArch::Amd64).expect("lowering should succeed");

        let allocated =
            linear_scan_allocate(&mir, TargetArch::Amd64).expect("allocation should succeed");
        let asm =
            emit_x86_64_assembly(&allocated.function).expect("assembly emission should succeed");
        assert!(asm.contains("edge_"));
        assert!(asm.contains("imulq"));
    }

    #[test]
    fn end_to_end_main_return_42_pipeline() {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder
            .position_at_end(entry)
            .expect("entry block should exist");
        let value_42 = builder
            .build_const_i32(42)
            .expect("constant build should succeed");
        builder
            .build_ret(Some(value_42))
            .expect("return build should succeed");
        let function = builder.finish();

        let lowered = lower_il_to_mir(&function, TargetArch::Amd64).expect("lowering should work");
        let allocated =
            linear_scan_allocate(&lowered, TargetArch::Amd64).expect("allocation should work");
        let asm = emit_x86_64_assembly(&allocated.function).expect("emission should work");

        assert!(asm.contains("movq $42"));
        assert!(asm.contains("ret"));
    }

    #[test]
    fn lowers_alloca_load_store_and_sdiv_and() {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder
            .position_at_end(entry)
            .expect("entry block should exist");

        let ptr = builder
            .build_alloca(Type::I32)
            .expect("alloca should be valid");
        let value_84 = builder.build_const_i32(84).expect("constant should build");
        builder
            .build_store(Type::I32, value_84, ptr)
            .expect("store should build");

        let loaded = builder
            .build_load(Type::I32, ptr)
            .expect("load should build");
        let two = builder.build_const_i32(2).expect("constant should build");
        let divided = builder.build_sdiv(loaded, two).expect("sdiv should build");
        let mask = builder.build_const_i32(31).expect("constant should build");
        let masked = builder.build_and(divided, mask).expect("and should build");
        builder
            .build_ret(Some(masked))
            .expect("return should build");

        let function = builder.finish();
        let lowered = lower_il_to_mir(&function, TargetArch::Amd64).expect("lowering should work");
        let allocated =
            linear_scan_allocate(&lowered, TargetArch::Amd64).expect("allocation should work");
        let asm = emit_x86_64_assembly(&allocated.function).expect("emission should work");

        assert!(asm.contains("idivq"));
        assert!(asm.contains("andq"));
        assert!(asm.contains("(%rbp)"));
    }

    #[test]
    fn lowers_function_param_from_abi_arg_register() {
        let mut builder = IrBuilder::new("identity", Type::I32);
        let n = builder.add_param("n", Type::I32);
        let entry = builder.create_block("entry");
        builder
            .position_at_end(entry)
            .expect("entry block should exist");
        builder.build_ret(Some(n)).expect("ret should build");

        let function = builder.finish();
        let lowered = lower_il_to_mir(&function, TargetArch::Amd64).expect("lowering should work");

        assert!(lowered.instructions.iter().any(|inst| {
            matches!(
                inst,
                MirInst::Mov {
                    dst: Reg::VReg(_),
                    src: Operand::Reg(Reg::Phys(PhysReg::RDI)),
                }
            )
        }));
    }

    #[test]
    fn lowers_call_args_via_stack_staging() {
        let mut builder = IrBuilder::new("caller", Type::I32);
        let a = builder.add_param("a", Type::I32);
        let b = builder.add_param("b", Type::I32);
        let entry = builder.create_block("entry");
        builder
            .position_at_end(entry)
            .expect("entry block should exist");

        let call_result = builder
            .build_call(Type::I32, "add2", vec![(Type::I32, b), (Type::I32, a)])
            .expect("call should build")
            .expect("call should return a value");
        builder
            .build_ret(Some(call_result))
            .expect("ret should build");

        let function = builder.finish();
        let lowered = lower_il_to_mir(&function, TargetArch::Amd64).expect("lowering should work");

        let call_index = lowered
            .instructions
            .iter()
            .position(|inst| matches!(inst, MirInst::Call { .. }))
            .expect("call should be present");
        let before_call = &lowered.instructions[..call_index];

        let staged_store_count = before_call
            .iter()
            .filter(|inst| matches!(inst, MirInst::StoreStack { .. }))
            .count();
        assert!(
            staged_store_count >= 2,
            "expected args to be staged into stack slots before call"
        );

        assert!(before_call.iter().any(|inst| {
            matches!(
                inst,
                MirInst::LoadStack {
                    dst: Reg::Phys(PhysReg::RDI),
                    ..
                }
            )
        }));
        assert!(before_call.iter().any(|inst| {
            matches!(
                inst,
                MirInst::LoadStack {
                    dst: Reg::Phys(PhysReg::RSI),
                    ..
                }
            )
        }));
    }
}
