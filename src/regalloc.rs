use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use crate::mir::{MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveInterval {
    pub vreg: usize,
    pub start: usize,
    pub end: usize,
}

impl LiveInterval {
    fn contains(self, index: usize) -> bool {
        self.start <= index && index <= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VRegAllocation {
    pub reg: Option<PhysReg>,
    pub stack_offset: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearScanAllocation {
    pub function: MirFunction,
    pub intervals: Vec<LiveInterval>,
    pub allocations: BTreeMap<usize, VRegAllocation>,
    pub stack_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocError {
    message: String,
}

impl AllocError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for AllocError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for AllocError {}

#[derive(Debug, Clone, Copy, Default)]
struct WorkingAlloc {
    reg: Option<PhysReg>,
    stack_offset: Option<i32>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveInterval {
    vreg: usize,
    end: usize,
    reg: PhysReg,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedReg {
    reg: Reg,
    is_temp: bool,
}

#[derive(Debug, Clone)]
struct ResolvedOperand {
    operand: Operand,
}

pub fn compute_live_intervals(func: &MirFunction) -> Vec<LiveInterval> {
    if func.instructions.is_empty() {
        return Vec::new();
    }

    let mut label_to_index = HashMap::new();
    for (index, inst) in func.instructions.iter().enumerate() {
        if let MirInst::Label(label) = inst {
            label_to_index.insert(label.clone(), index);
        }
    }

    let mut uses = vec![Vec::new(); func.instructions.len()];
    let mut defs = vec![Vec::new(); func.instructions.len()];
    for (index, inst) in func.instructions.iter().enumerate() {
        collect_vreg_uses(inst, &mut uses[index]);
        collect_vreg_defs(inst, &mut defs[index]);
    }

    let mut live_in: Vec<HashSet<usize>> = vec![HashSet::new(); func.instructions.len()];
    let mut live_out: Vec<HashSet<usize>> = vec![HashSet::new(); func.instructions.len()];

    loop {
        let mut changed = false;

        for index in (0..func.instructions.len()).rev() {
            let mut next_out = HashSet::new();
            for succ in instruction_successors(index, &func.instructions, &label_to_index) {
                next_out.extend(live_in[succ].iter().copied());
            }

            let mut next_in = next_out.clone();
            for def in &defs[index] {
                next_in.remove(def);
            }
            for used in &uses[index] {
                next_in.insert(*used);
            }

            if next_in != live_in[index] || next_out != live_out[index] {
                live_in[index] = next_in;
                live_out[index] = next_out;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    let mut ranges: HashMap<usize, (usize, usize)> = HashMap::new();
    for index in 0..func.instructions.len() {
        for vreg in live_in[index]
            .iter()
            .chain(live_out[index].iter())
            .chain(uses[index].iter())
            .chain(defs[index].iter())
        {
            let entry = ranges.entry(*vreg).or_insert((index, index));
            entry.0 = entry.0.min(index);
            entry.1 = entry.1.max(index);
        }
    }

    let mut intervals: Vec<_> = ranges
        .into_iter()
        .map(|(vreg, (start, end))| LiveInterval { start, end, vreg })
        .collect();
    intervals.sort_by_key(|interval| (interval.start, interval.vreg));
    intervals
}

fn instruction_successors(
    index: usize,
    instructions: &[MirInst],
    label_to_index: &HashMap<String, usize>,
) -> Vec<usize> {
    match &instructions[index] {
        MirInst::Ret => Vec::new(),
        MirInst::Jmp { label } => label_to_index.get(label).copied().into_iter().collect(),
        MirInst::JmpIf { label, .. } => {
            let mut succ = Vec::new();
            if let Some(target) = label_to_index.get(label) {
                succ.push(*target);
            }
            if index + 1 < instructions.len() {
                succ.push(index + 1);
            }
            succ
        }
        _ => {
            if index + 1 < instructions.len() {
                vec![index + 1]
            } else {
                Vec::new()
            }
        }
    }
}

pub fn linear_scan_allocate(
    func: &MirFunction,
    target: TargetArch,
) -> Result<LinearScanAllocation, AllocError> {
    match target {
        TargetArch::Amd64 => linear_scan_allocate_x86_64(func),
        TargetArch::X86_64 => linear_scan_allocate_x86_64(func),
        TargetArch::Arm64 => linear_scan_allocate_arm64(func),
    }
}

fn linear_scan_allocate_arm64(func: &MirFunction) -> Result<LinearScanAllocation, AllocError> {
    let intervals = compute_live_intervals(func);
    let allocatable = allocatable_arm64_registers();
    let preexisting_stack = max_preexisting_stack_offset(func);

    let mut working: HashMap<usize, WorkingAlloc> = HashMap::new();
    let mut active: Vec<ActiveInterval> = Vec::new();
    let mut free_regs = allocatable.to_vec();
    let mut next_stack_offset: i32 = preexisting_stack;

    for interval in &intervals {
        expire_old_intervals(interval.start, &mut active, &mut free_regs, allocatable);

        if active.len() >= allocatable.len() {
            let (spill_index, spill_candidate) = active
                .iter()
                .enumerate()
                .max_by_key(|(_, candidate)| candidate.end)
                .map(|(index, candidate)| (index, *candidate))
                .ok_or_else(|| AllocError::new("internal allocator error: no active candidate"))?;

            if spill_candidate.end > interval.end {
                active.remove(spill_index);

                let spilled = working.entry(spill_candidate.vreg).or_default();
                spilled.reg = None;
                ensure_stack_slot(spill_candidate.vreg, &mut working, &mut next_stack_offset);

                let current = working.entry(interval.vreg).or_default();
                current.reg = Some(spill_candidate.reg);

                active.push(ActiveInterval {
                    vreg: interval.vreg,
                    end: interval.end,
                    reg: spill_candidate.reg,
                });
                active.sort_by_key(|candidate| candidate.end);
            } else {
                let current = working.entry(interval.vreg).or_default();
                current.reg = None;
                ensure_stack_slot(interval.vreg, &mut working, &mut next_stack_offset);
            }
        } else {
            let reg = free_regs.first().copied().ok_or_else(|| {
                AllocError::new("internal allocator error: free register missing")
            })?;
            free_regs.remove(0);

            let current = working.entry(interval.vreg).or_default();
            current.reg = Some(reg);

            active.push(ActiveInterval {
                vreg: interval.vreg,
                end: interval.end,
                reg,
            });
            active.sort_by_key(|candidate| candidate.end);
        }
    }

    // Any caller-saved vreg live across a call gets a stack home so we can
    // explicitly evict/restore around call sites.
    for (index, inst) in func.instructions.iter().enumerate() {
        if !matches!(inst, MirInst::Call { .. }) {
            continue;
        }

        for interval in &intervals {
            if !interval.contains(index) {
                continue;
            }

            let reg = working
                .get(&interval.vreg)
                .ok_or_else(|| AllocError::new("interval has no allocation state"))?
                .reg;
            if let Some(reg) = reg
                && is_arm64_caller_saved(reg)
            {
                if reg == PhysReg::X0 {
                    // Call results are returned in X0. Keeping a live-across-call
                    // value in X0 would force a restore that can clobber the
                    // callee return value before it is consumed.
                    let state = working
                        .get_mut(&interval.vreg)
                        .ok_or_else(|| AllocError::new("interval has no allocation state"))?;
                    state.reg = None;
                }

                ensure_stack_slot(interval.vreg, &mut working, &mut next_stack_offset);
            }
        }
    }

    let has_call = func
        .instructions
        .iter()
        .any(|inst| matches!(inst, MirInst::Call { .. }));
    let raw_stack_size = next_stack_offset.max(0) as usize;
    let (saved_fp_offset, saved_lr_offset, stack_size) = if raw_stack_size > 0 || has_call {
        // Reserve frame-pointer and link-register spill slots beyond all user/lowering stack slots.
        let saved_fp_offset = next_stack_offset + 8;
        let saved_lr_offset = next_stack_offset + 16;
        let raw_with_frame = saved_lr_offset as usize;
        let stack_size = ((raw_with_frame + 15) / 16) * 16;
        (Some(saved_fp_offset), Some(saved_lr_offset), stack_size)
    } else {
        (None, None, 0)
    };

    let lowered = rewrite_with_allocations_arm64(
        func,
        &intervals,
        &working,
        stack_size,
        saved_fp_offset,
        saved_lr_offset,
    )?;
    let allocated_func = MirFunction::with_instructions(func.name.clone(), lowered);

    let mut allocations = BTreeMap::new();
    for (vreg, state) in &working {
        allocations.insert(
            *vreg,
            VRegAllocation {
                reg: state.reg,
                stack_offset: state.stack_offset,
            },
        );
    }

    Ok(LinearScanAllocation {
        function: allocated_func,
        intervals,
        allocations,
        stack_size,
    })
}

fn rewrite_with_allocations_arm64(
    func: &MirFunction,
    intervals: &[LiveInterval],
    allocations: &HashMap<usize, WorkingAlloc>,
    stack_size: usize,
    saved_fp_offset: Option<i32>,
    saved_lr_offset: Option<i32>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();
    let has_frame = stack_size > 0;

    if has_frame {
        let saved_fp_offset = saved_fp_offset
            .ok_or_else(|| AllocError::new("missing saved frame-pointer slot for arm64"))?;
        let saved_lr_offset = saved_lr_offset
            .ok_or_else(|| AllocError::new("missing saved link-register slot for arm64"))?;

        // Preserve caller frame pointer before we repurpose x29 as our frame base.
        out.push(MirInst::Mov {
            dst: Reg::Phys(PhysReg::X16),
            src: Operand::Reg(Reg::Phys(PhysReg::X29)),
        });
        out.push(MirInst::Sub {
            dst: Reg::Phys(PhysReg::SP),
            lhs: Reg::Phys(PhysReg::SP),
            rhs: Operand::Imm(stack_size as i64),
        });
        out.push(MirInst::Add {
            dst: Reg::Phys(PhysReg::X29),
            lhs: Reg::Phys(PhysReg::SP),
            rhs: Operand::Imm(stack_size as i64),
        });
        out.push(MirInst::StoreStack {
            src: Reg::Phys(PhysReg::X16),
            offset: saved_fp_offset,
        });
        out.push(MirInst::StoreStack {
            src: Reg::Phys(PhysReg::X30),
            offset: saved_lr_offset,
        });
    }

    for (index, inst) in func.instructions.iter().enumerate() {
        match inst {
            MirInst::Label(label) => out.push(MirInst::Label(label.clone())),
            MirInst::Mov { dst, src } => {
                out.extend(lower_mov_arm64(*dst, src, allocations)?);
            }
            MirInst::Add { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_arm64(
                    ArithOp::Add,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Sub { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_arm64(
                    ArithOp::Sub,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::And { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_arm64(
                    ArithOp::And,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Mul { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_arm64(
                    ArithOp::Mul,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Sdiv { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_arm64(
                    ArithOp::Sdiv,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Cmp { lhs, rhs } => {
                out.extend(lower_cmp_arm64(*lhs, rhs, allocations)?);
            }
            MirInst::LoadStack { dst, offset } => {
                out.extend(lower_load_stack_arm64(*dst, *offset, allocations)?);
            }
            MirInst::StoreStack { src, offset } => {
                out.extend(lower_store_stack_arm64(*src, *offset, allocations)?);
            }
            MirInst::Jmp { label } => out.push(MirInst::Jmp {
                label: label.clone(),
            }),
            MirInst::JmpIf { cond, label } => out.push(MirInst::JmpIf {
                cond: *cond,
                label: label.clone(),
            }),
            MirInst::Call { symbol } => {
                let mut live = Vec::new();
                for interval in intervals {
                    if !interval.contains(index) {
                        continue;
                    }

                    if let Some(state) = allocations.get(&interval.vreg)
                        && let (Some(reg), Some(offset)) = (state.reg, state.stack_offset)
                        && is_arm64_caller_saved(reg)
                    {
                        live.push((interval.vreg, reg, offset, interval.end > index));
                    }
                }

                live.sort_by_key(|(vreg, _, _, _)| *vreg);

                for (_, reg, offset, _) in &live {
                    out.push(MirInst::StoreStack {
                        src: Reg::Phys(*reg),
                        offset: *offset,
                    });
                }

                out.push(MirInst::Call {
                    symbol: symbol.clone(),
                });

                for (_, reg, offset, needs_restore) in &live {
                    if !needs_restore {
                        continue;
                    }
                    out.push(MirInst::LoadStack {
                        dst: Reg::Phys(*reg),
                        offset: *offset,
                    });
                }
            }
            MirInst::Ret => {
                if has_frame {
                    let saved_fp_offset = saved_fp_offset.ok_or_else(|| {
                        AllocError::new("missing saved frame-pointer slot for arm64")
                    })?;
                    let saved_lr_offset = saved_lr_offset.ok_or_else(|| {
                        AllocError::new("missing saved link-register slot for arm64")
                    })?;

                    out.push(MirInst::LoadStack {
                        dst: Reg::Phys(PhysReg::X30),
                        offset: saved_lr_offset,
                    });
                    out.push(MirInst::LoadStack {
                        dst: Reg::Phys(PhysReg::X29),
                        offset: saved_fp_offset,
                    });
                    out.push(MirInst::Add {
                        dst: Reg::Phys(PhysReg::SP),
                        lhs: Reg::Phys(PhysReg::SP),
                        rhs: Operand::Imm(stack_size as i64),
                    });
                }
                out.push(MirInst::Ret);
            }
            MirInst::Push { .. } | MirInst::Pop { .. } => {
                return Err(AllocError::new(
                    "push/pop pseudo-instructions are not supported in arm64 allocation",
                ));
            }
        }
    }

    Ok(peephole_optimize_arm64(out))
}

fn peephole_optimize_arm64(instructions: Vec<MirInst>) -> Vec<MirInst> {
    let mut simplified = Vec::with_capacity(instructions.len());
    let mut index = 0;

    while index < instructions.len() {
        if let Some((rewritten, consumed)) = try_fold_arm64_arith_result_move(&instructions, index)
        {
            simplified.push(rewritten);
            index += consumed;
            continue;
        }

        match &instructions[index] {
            MirInst::Mov {
                dst,
                src: Operand::Reg(src_reg),
            } => {
                if dst == src_reg {
                    index += 1;
                    continue;
                }

                if index + 1 < instructions.len()
                    && let MirInst::Mov {
                        dst: next_dst,
                        src: Operand::Reg(next_src),
                    } = &instructions[index + 1]
                    && next_dst == src_reg
                    && next_src == dst
                {
                    simplified.push(instructions[index].clone());
                    index += 2;
                    continue;
                }

                simplified.push(instructions[index].clone());
                index += 1;
            }
            _ => {
                simplified.push(instructions[index].clone());
                index += 1;
            }
        }
    }

    let mut out = Vec::with_capacity(simplified.len());
    for (idx, inst) in simplified.iter().enumerate() {
        let is_dead_move = matches!(
            inst,
            MirInst::Mov {
                dst,
                src: Operand::Reg(_)
            } if move_destination_is_dead_arm64(&simplified, idx, *dst)
        );

        if !is_dead_move {
            out.push(inst.clone());
        }
    }

    out
}

fn try_fold_arm64_arith_result_move(
    instructions: &[MirInst],
    index: usize,
) -> Option<(MirInst, usize)> {
    let current = instructions.get(index)?;
    let next = instructions.get(index + 1)?;

    let arith_dst = match current {
        MirInst::Add { dst, .. }
        | MirInst::Sub { dst, .. }
        | MirInst::And { dst, .. }
        | MirInst::Mul { dst, .. }
        | MirInst::Sdiv { dst, .. } => *dst,
        _ => return None,
    };

    let MirInst::Mov {
        dst: mov_dst,
        src: Operand::Reg(mov_src),
    } = next
    else {
        return None;
    };

    if *mov_src != arith_dst {
        return None;
    }

    if !move_destination_is_dead_arm64(instructions, index + 1, arith_dst) {
        return None;
    }

    let rewritten = match current {
        MirInst::Add { lhs, rhs, .. } => MirInst::Add {
            dst: *mov_dst,
            lhs: *lhs,
            rhs: rhs.clone(),
        },
        MirInst::Sub { lhs, rhs, .. } => MirInst::Sub {
            dst: *mov_dst,
            lhs: *lhs,
            rhs: rhs.clone(),
        },
        MirInst::And { lhs, rhs, .. } => MirInst::And {
            dst: *mov_dst,
            lhs: *lhs,
            rhs: rhs.clone(),
        },
        MirInst::Mul { lhs, rhs, .. } => MirInst::Mul {
            dst: *mov_dst,
            lhs: *lhs,
            rhs: rhs.clone(),
        },
        MirInst::Sdiv { lhs, rhs, .. } => MirInst::Sdiv {
            dst: *mov_dst,
            lhs: *lhs,
            rhs: rhs.clone(),
        },
        _ => return None,
    };

    Some((rewritten, 2))
}

fn move_destination_is_dead_arm64(instructions: &[MirInst], move_index: usize, dst: Reg) -> bool {
    for inst in instructions.iter().skip(move_index + 1) {
        match inst {
            MirInst::Label(_) | MirInst::Jmp { .. } | MirInst::JmpIf { .. } | MirInst::Call { .. } => {
                return false;
            }
            MirInst::Ret => {
                return dst != Reg::Phys(PhysReg::X0);
            }
            _ => {
                if inst_reads_reg_arm64(inst, dst) {
                    return false;
                }
                if inst_writes_reg_arm64(inst, dst) {
                    return true;
                }
            }
        }
    }

    false
}

fn inst_reads_reg_arm64(inst: &MirInst, reg: Reg) -> bool {
    match inst {
        MirInst::Mov { src, .. } => matches!(src, Operand::Reg(src_reg) if *src_reg == reg),
        MirInst::Add { lhs, rhs, .. }
        | MirInst::Sub { lhs, rhs, .. }
        | MirInst::And { lhs, rhs, .. }
        | MirInst::Mul { lhs, rhs, .. }
        | MirInst::Sdiv { lhs, rhs, .. } => {
            *lhs == reg || matches!(rhs, Operand::Reg(rhs_reg) if *rhs_reg == reg)
        }
        MirInst::Cmp { lhs, rhs } => {
            *lhs == reg || matches!(rhs, Operand::Reg(rhs_reg) if *rhs_reg == reg)
        }
        MirInst::Push { src } | MirInst::StoreStack { src, .. } => *src == reg,
        MirInst::Label(_)
        | MirInst::Pop { .. }
        | MirInst::LoadStack { .. }
        | MirInst::Jmp { .. }
        | MirInst::JmpIf { .. }
        | MirInst::Call { .. }
        | MirInst::Ret => false,
    }
}

fn inst_writes_reg_arm64(inst: &MirInst, reg: Reg) -> bool {
    match inst {
        MirInst::Mov { dst, .. }
        | MirInst::Add { dst, .. }
        | MirInst::Sub { dst, .. }
        | MirInst::And { dst, .. }
        | MirInst::Mul { dst, .. }
        | MirInst::Sdiv { dst, .. }
        | MirInst::LoadStack { dst, .. }
        | MirInst::Pop { dst } => *dst == reg,
        MirInst::Label(_)
        | MirInst::Cmp { .. }
        | MirInst::Push { .. }
        | MirInst::StoreStack { .. }
        | MirInst::Jmp { .. }
        | MirInst::JmpIf { .. }
        | MirInst::Call { .. }
        | MirInst::Ret => false,
    }
}

fn lower_mov_arm64(
    dst: Reg,
    src: &Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();
    let resolved_src = resolve_operand_use_arm64(src.clone(), allocations, &mut out)?;

    match dst {
        Reg::Phys(phys) => {
            let dst_reg = Reg::Phys(phys);
            if !is_identity_move(dst_reg, &resolved_src) {
                out.push(MirInst::Mov {
                    dst: dst_reg,
                    src: resolved_src,
                });
            }
        }
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                let dst_reg = Reg::Phys(phys);
                if !is_identity_move(dst_reg, &resolved_src) {
                    out.push(MirInst::Mov {
                        dst: dst_reg,
                        src: resolved_src,
                    });
                }
            } else {
                let slot = alloc
                    .stack_offset
                    .ok_or_else(|| AllocError::new("spilled arm64 vreg is missing a stack slot"))?;

                match resolved_src {
                    Operand::Reg(Reg::Phys(src_phys)) => {
                        out.push(MirInst::StoreStack {
                            src: Reg::Phys(src_phys),
                            offset: slot,
                        });
                    }
                    Operand::Imm(imm) => {
                        out.push(MirInst::Mov {
                            dst: Reg::Phys(PhysReg::X16),
                            src: Operand::Imm(imm),
                        });
                        out.push(MirInst::StoreStack {
                            src: Reg::Phys(PhysReg::X16),
                            offset: slot,
                        });
                    }
                    Operand::Reg(Reg::VReg(_)) => {
                        return Err(AllocError::new(
                            "resolved arm64 operand still contains virtual register",
                        ));
                    }
                }
            }
        }
    }

    Ok(out)
}

fn lower_arithmetic_arm64(
    op: ArithOp,
    dst: Reg,
    lhs: Reg,
    rhs: &Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();

    let lhs_reg = resolve_use_reg_arm64(lhs, allocations, &mut out, PhysReg::X16)?;
    let mut rhs_operand = resolve_operand_use_arm64(rhs.clone(), allocations, &mut out)?;

    if matches!(op, ArithOp::And | ArithOp::Mul | ArithOp::Sdiv)
        && let Operand::Imm(imm) = rhs_operand
    {
        out.push(MirInst::Mov {
            dst: Reg::Phys(PhysReg::X17),
            src: Operand::Imm(imm),
        });
        rhs_operand = Operand::Reg(Reg::Phys(PhysReg::X17));
    }

    let (dst_reg, dst_slot) = match dst {
        Reg::Phys(phys) => (Reg::Phys(phys), None),
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                (Reg::Phys(phys), None)
            } else {
                let slot = alloc.stack_offset.ok_or_else(|| {
                    AllocError::new("spilled arm64 vreg is missing a stack slot")
                })?;
                (Reg::Phys(PhysReg::X15), Some(slot))
            }
        }
    };

    out.push(match op {
        ArithOp::Add => MirInst::Add {
            dst: dst_reg,
            lhs: lhs_reg,
            rhs: rhs_operand,
        },
        ArithOp::Sub => MirInst::Sub {
            dst: dst_reg,
            lhs: lhs_reg,
            rhs: rhs_operand,
        },
        ArithOp::And => MirInst::And {
            dst: dst_reg,
            lhs: lhs_reg,
            rhs: rhs_operand,
        },
        ArithOp::Mul => MirInst::Mul {
            dst: dst_reg,
            lhs: lhs_reg,
            rhs: rhs_operand,
        },
        ArithOp::Sdiv => MirInst::Sdiv {
            dst: dst_reg,
            lhs: lhs_reg,
            rhs: rhs_operand,
        },
    });

    if let Some(slot) = dst_slot {
        out.push(MirInst::StoreStack {
            src: dst_reg,
            offset: slot,
        });
    }

    Ok(out)
}

fn lower_cmp_arm64(
    lhs: Reg,
    rhs: &Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();
    let lhs_reg = resolve_use_reg_arm64(lhs, allocations, &mut out, PhysReg::X16)?;
    let rhs_operand = resolve_operand_use_arm64(rhs.clone(), allocations, &mut out)?;
    out.push(MirInst::Cmp {
        lhs: lhs_reg,
        rhs: rhs_operand,
    });
    Ok(out)
}

fn lower_load_stack_arm64(
    dst: Reg,
    offset: i32,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();

    match dst {
        Reg::Phys(phys) => out.push(MirInst::LoadStack {
            dst: Reg::Phys(phys),
            offset,
        }),
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(phys),
                    offset,
                });
            } else {
                let slot = alloc.stack_offset.ok_or_else(|| {
                    AllocError::new("spilled arm64 vreg is missing a stack slot")
                })?;
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(PhysReg::X16),
                    offset,
                });
                out.push(MirInst::StoreStack {
                    src: Reg::Phys(PhysReg::X16),
                    offset: slot,
                });
            }
        }
    }

    Ok(out)
}

fn lower_store_stack_arm64(
    src: Reg,
    offset: i32,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();

    match src {
        Reg::Phys(phys) => out.push(MirInst::StoreStack {
            src: Reg::Phys(phys),
            offset,
        }),
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                out.push(MirInst::StoreStack {
                    src: Reg::Phys(phys),
                    offset,
                });
            } else {
                let slot = alloc.stack_offset.ok_or_else(|| {
                    AllocError::new("spilled arm64 vreg is missing a stack slot")
                })?;
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(PhysReg::X16),
                    offset: slot,
                });
                out.push(MirInst::StoreStack {
                    src: Reg::Phys(PhysReg::X16),
                    offset,
                });
            }
        }
    }

    Ok(out)
}

fn resolve_use_reg_arm64(
    reg: Reg,
    allocations: &HashMap<usize, WorkingAlloc>,
    out: &mut Vec<MirInst>,
    scratch: PhysReg,
) -> Result<Reg, AllocError> {
    match reg {
        Reg::Phys(phys) => Ok(Reg::Phys(phys)),
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                Ok(Reg::Phys(phys))
            } else {
                let slot = alloc.stack_offset.ok_or_else(|| {
                    AllocError::new("spilled arm64 vreg is missing a stack slot")
                })?;
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(scratch),
                    offset: slot,
                });
                Ok(Reg::Phys(scratch))
            }
        }
    }
}

fn resolve_operand_use_arm64(
    operand: Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
    out: &mut Vec<MirInst>,
) -> Result<Operand, AllocError> {
    match operand {
        Operand::Imm(imm) => Ok(Operand::Imm(imm)),
        Operand::Reg(reg) => {
            let resolved = resolve_use_reg_arm64(reg, allocations, out, PhysReg::X17)?;
            Ok(Operand::Reg(resolved))
        }
    }
}

fn linear_scan_allocate_x86_64(func: &MirFunction) -> Result<LinearScanAllocation, AllocError> {
    let intervals = compute_live_intervals(func);
    let allocatable = allocatable_x86_64_registers();
    let preexisting_stack = max_preexisting_stack_offset(func);

    let mut working: HashMap<usize, WorkingAlloc> = HashMap::new();
    let mut active: Vec<ActiveInterval> = Vec::new();
    let mut free_regs = allocatable.to_vec();
    let mut next_stack_offset: i32 = preexisting_stack;

    for interval in &intervals {
        expire_old_intervals(interval.start, &mut active, &mut free_regs, allocatable);

        if active.len() >= allocatable.len() {
            let (spill_index, spill_candidate) = active
                .iter()
                .enumerate()
                .max_by_key(|(_, candidate)| candidate.end)
                .map(|(index, candidate)| (index, *candidate))
                .ok_or_else(|| AllocError::new("internal allocator error: no active candidate"))?;

            if spill_candidate.end > interval.end {
                active.remove(spill_index);

                let spilled = working.entry(spill_candidate.vreg).or_default();
                spilled.reg = None;
                ensure_stack_slot(spill_candidate.vreg, &mut working, &mut next_stack_offset);

                let current = working.entry(interval.vreg).or_default();
                current.reg = Some(spill_candidate.reg);

                active.push(ActiveInterval {
                    vreg: interval.vreg,
                    end: interval.end,
                    reg: spill_candidate.reg,
                });
                active.sort_by_key(|candidate| candidate.end);
            } else {
                let current = working.entry(interval.vreg).or_default();
                current.reg = None;
                ensure_stack_slot(interval.vreg, &mut working, &mut next_stack_offset);
            }
        } else {
            let reg = free_regs.first().copied().ok_or_else(|| {
                AllocError::new("internal allocator error: free register missing")
            })?;
            free_regs.remove(0);

            let current = working.entry(interval.vreg).or_default();
            current.reg = Some(reg);

            active.push(ActiveInterval {
                vreg: interval.vreg,
                end: interval.end,
                reg,
            });
            active.sort_by_key(|candidate| candidate.end);
        }
    }

    // Any caller-saved vreg live across a call gets a stack home so we can
    // explicitly evict/restore around call sites.
    for (index, inst) in func.instructions.iter().enumerate() {
        if !matches!(inst, MirInst::Call { .. }) {
            continue;
        }

        for interval in &intervals {
            if !interval.contains(index) {
                continue;
            }

            let reg = working
                .get(&interval.vreg)
                .ok_or_else(|| AllocError::new("interval has no allocation state"))?
                .reg;
            if let Some(reg) = reg
                && is_x86_64_caller_saved(reg)
            {
                ensure_stack_slot(interval.vreg, &mut working, &mut next_stack_offset);
            }
        }
    }

    let callee_saved = collect_used_callee_saved_registers(&working);
    let raw_stack_size = next_stack_offset.max(0) as usize;
    let stack_size = align_stack_size_for_calls(raw_stack_size, callee_saved.len());

    let lowered =
        rewrite_with_allocations_x86_64(func, &intervals, &working, stack_size, &callee_saved)?;
    let allocated_func = MirFunction::with_instructions(func.name.clone(), lowered);

    let mut allocations = BTreeMap::new();
    for (vreg, state) in &working {
        allocations.insert(
            *vreg,
            VRegAllocation {
                reg: state.reg,
                stack_offset: state.stack_offset,
            },
        );
    }

    Ok(LinearScanAllocation {
        function: allocated_func,
        intervals,
        allocations,
        stack_size,
    })
}

fn rewrite_with_allocations_x86_64(
    func: &MirFunction,
    intervals: &[LiveInterval],
    allocations: &HashMap<usize, WorkingAlloc>,
    stack_size: usize,
    callee_saved: &[PhysReg],
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();
    let has_call = func
        .instructions
        .iter()
        .any(|inst| matches!(inst, MirInst::Call { .. }));
    let omit_frame = stack_size == 0 && callee_saved.is_empty() && !has_call;

    if !omit_frame {
        out.push(MirInst::Push {
            src: Reg::Phys(PhysReg::RBP),
        });
        for reg in callee_saved {
            out.push(MirInst::Push {
                src: Reg::Phys(*reg),
            });
        }
        out.push(MirInst::Mov {
            dst: Reg::Phys(PhysReg::RBP),
            src: Operand::Reg(Reg::Phys(PhysReg::RSP)),
        });
        if stack_size > 0 {
            out.push(MirInst::Sub {
                dst: Reg::Phys(PhysReg::RSP),
                lhs: Reg::Phys(PhysReg::RSP),
                rhs: Operand::Imm(stack_size as i64),
            });
        }
    }

    for (index, inst) in func.instructions.iter().enumerate() {
        match inst {
            MirInst::Label(label) => out.push(MirInst::Label(label.clone())),
            MirInst::Mov { dst, src } => {
                out.extend(lower_mov_x86_64(*dst, src, allocations)?);
            }
            MirInst::Add { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_x86_64(
                    ArithOp::Add,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Sub { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_x86_64(
                    ArithOp::Sub,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::And { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_x86_64(
                    ArithOp::And,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Mul { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_x86_64(
                    ArithOp::Mul,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Sdiv { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_x86_64(
                    ArithOp::Sdiv,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Cmp { lhs, rhs } => {
                out.extend(lower_cmp_x86_64(*lhs, rhs, allocations)?);
            }
            MirInst::LoadStack { dst, offset } => {
                out.extend(lower_load_stack_x86_64(*dst, *offset, allocations)?);
            }
            MirInst::StoreStack { src, offset } => {
                out.extend(lower_store_stack_x86_64(*src, *offset, allocations)?);
            }
            MirInst::Jmp { label } => out.push(MirInst::Jmp {
                label: label.clone(),
            }),
            MirInst::JmpIf { cond, label } => out.push(MirInst::JmpIf {
                cond: *cond,
                label: label.clone(),
            }),
            MirInst::Call { symbol } => {
                let mut live = Vec::new();
                for interval in intervals {
                    if !interval.contains(index) {
                        continue;
                    }

                    if let Some(state) = allocations.get(&interval.vreg)
                        && let (Some(reg), Some(offset)) = (state.reg, state.stack_offset)
                        && is_x86_64_caller_saved(reg)
                    {
                        live.push((interval.vreg, reg, offset, interval.end > index));
                    }
                }

                live.sort_by_key(|(vreg, _, _, _)| *vreg);

                for (_, reg, offset, _) in &live {
                    out.push(MirInst::StoreStack {
                        src: Reg::Phys(*reg),
                        offset: *offset,
                    });
                }

                out.push(MirInst::Call {
                    symbol: symbol.clone(),
                });

                for (_, reg, offset, needs_restore) in &live {
                    if !needs_restore {
                        continue;
                    }
                    out.push(MirInst::LoadStack {
                        dst: Reg::Phys(*reg),
                        offset: *offset,
                    });
                }
            }
            MirInst::Ret => {
                if !omit_frame {
                    if stack_size > 0 {
                        out.push(MirInst::Add {
                            dst: Reg::Phys(PhysReg::RSP),
                            lhs: Reg::Phys(PhysReg::RSP),
                            rhs: Operand::Imm(stack_size as i64),
                        });
                    }
                    for reg in callee_saved.iter().rev() {
                        out.push(MirInst::Pop {
                            dst: Reg::Phys(*reg),
                        });
                    }
                    out.push(MirInst::Pop {
                        dst: Reg::Phys(PhysReg::RBP),
                    });
                }
                out.push(MirInst::Ret);
            }
            MirInst::Push { src } | MirInst::Pop { dst: src } => {
                if contains_vreg(*src) {
                    return Err(AllocError::new(
                        "input MIR must not contain stack/push/pop ops with virtual registers",
                    ));
                }
                out.push(inst.clone());
            }
        }
    }

    Ok(peephole_optimize_x86_64(out))
}

fn peephole_optimize_x86_64(instructions: Vec<MirInst>) -> Vec<MirInst> {
    let mut simplified = Vec::with_capacity(instructions.len());
    let mut index = 0;

    while index < instructions.len() {
        match &instructions[index] {
            MirInst::Mov {
                dst,
                src: Operand::Reg(src_reg),
            } => {
                if dst == src_reg {
                    index += 1;
                    continue;
                }

                if index + 1 < instructions.len()
                    && let MirInst::Mov {
                        dst: next_dst,
                        src: Operand::Reg(next_src),
                    } = &instructions[index + 1]
                    && next_dst == src_reg
                    && next_src == dst
                {
                    simplified.push(instructions[index].clone());
                    index += 2;
                    continue;
                }

                simplified.push(instructions[index].clone());
                index += 1;
            }
            _ => {
                simplified.push(instructions[index].clone());
                index += 1;
            }
        }
    }

    let mut out = Vec::with_capacity(simplified.len());
    for (idx, inst) in simplified.iter().enumerate() {
        let is_dead_move = matches!(
            inst,
            MirInst::Mov {
                dst,
                src: Operand::Reg(_)
            } if move_destination_is_dead_x86_64(&simplified, idx, *dst)
        );

        if !is_dead_move {
            out.push(inst.clone());
        }
    }

    out
}

fn move_destination_is_dead_x86_64(instructions: &[MirInst], move_index: usize, dst: Reg) -> bool {
    for inst in instructions.iter().skip(move_index + 1) {
        match inst {
            MirInst::Label(_) | MirInst::Jmp { .. } | MirInst::JmpIf { .. } | MirInst::Call { .. } => {
                return false;
            }
            MirInst::Ret => {
                return dst != Reg::Phys(PhysReg::RAX);
            }
            _ => {
                if inst_reads_reg_x86_64(inst, dst) {
                    return false;
                }
                if inst_writes_reg_x86_64(inst, dst) {
                    return true;
                }
            }
        }
    }

    false
}

fn inst_reads_reg_x86_64(inst: &MirInst, reg: Reg) -> bool {
    match inst {
        MirInst::Mov { src, .. } => matches!(src, Operand::Reg(src_reg) if *src_reg == reg),
        MirInst::Add { lhs, rhs, .. }
        | MirInst::Sub { lhs, rhs, .. }
        | MirInst::And { lhs, rhs, .. }
        | MirInst::Mul { lhs, rhs, .. }
        | MirInst::Sdiv { lhs, rhs, .. } => {
            *lhs == reg || matches!(rhs, Operand::Reg(rhs_reg) if *rhs_reg == reg)
        }
        MirInst::Cmp { lhs, rhs } => {
            *lhs == reg || matches!(rhs, Operand::Reg(rhs_reg) if *rhs_reg == reg)
        }
        MirInst::Push { src } | MirInst::StoreStack { src, .. } => *src == reg,
        MirInst::Label(_)
        | MirInst::Pop { .. }
        | MirInst::LoadStack { .. }
        | MirInst::Jmp { .. }
        | MirInst::JmpIf { .. }
        | MirInst::Call { .. }
        | MirInst::Ret => false,
    }
}

fn inst_writes_reg_x86_64(inst: &MirInst, reg: Reg) -> bool {
    match inst {
        MirInst::Mov { dst, .. }
        | MirInst::Add { dst, .. }
        | MirInst::Sub { dst, .. }
        | MirInst::And { dst, .. }
        | MirInst::Mul { dst, .. }
        | MirInst::Sdiv { dst, .. }
        | MirInst::LoadStack { dst, .. }
        | MirInst::Pop { dst } => *dst == reg,
        MirInst::Label(_)
        | MirInst::Cmp { .. }
        | MirInst::Push { .. }
        | MirInst::StoreStack { .. }
        | MirInst::Jmp { .. }
        | MirInst::JmpIf { .. }
        | MirInst::Call { .. }
        | MirInst::Ret => false,
    }
}

fn lower_mov_x86_64(
    dst: Reg,
    src: &Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let forbidden = collect_phys_regs_for_mov(dst, src);
    let mut used_scratch = Vec::new();
    let mut out = Vec::new();

    let resolved_src = resolve_operand_use_x86_64(
        src.clone(),
        allocations,
        &mut out,
        &mut used_scratch,
        &forbidden,
    )?;

    match dst {
        Reg::Phys(phys) => {
            let dst_reg = Reg::Phys(phys);
            if !is_identity_move(dst_reg, &resolved_src.operand) {
                out.push(MirInst::Mov {
                    dst: dst_reg,
                    src: resolved_src.operand,
                });
            }
        }
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                let dst_reg = Reg::Phys(phys);
                if !is_identity_move(dst_reg, &resolved_src.operand) {
                    out.push(MirInst::Mov {
                        dst: dst_reg,
                        src: resolved_src.operand,
                    });
                }
            } else {
                let offset = alloc
                    .stack_offset
                    .ok_or_else(|| AllocError::new("spilled vreg is missing a stack slot"))?;

                match resolved_src.operand {
                    Operand::Reg(Reg::Phys(src_phys)) => {
                        out.push(MirInst::StoreStack {
                            src: Reg::Phys(src_phys),
                            offset,
                        });
                    }
                    Operand::Imm(imm) => {
                        let scratch = alloc_scratch(&mut used_scratch, &forbidden)?;
                        out.push(MirInst::Mov {
                            dst: Reg::Phys(scratch),
                            src: Operand::Imm(imm),
                        });
                        out.push(MirInst::StoreStack {
                            src: Reg::Phys(scratch),
                            offset,
                        });
                    }
                    Operand::Reg(Reg::VReg(_)) => {
                        return Err(AllocError::new(
                            "resolved operand still contains virtual reg",
                        ));
                    }
                }
            }
        }
    }

    Ok(out)
}

fn is_identity_move(dst: Reg, src: &Operand) -> bool {
    matches!(src, Operand::Reg(src_reg) if *src_reg == dst)
}

fn lower_load_stack_x86_64(
    dst: Reg,
    offset: i32,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();

    match dst {
        Reg::Phys(phys) => {
            out.push(MirInst::LoadStack {
                dst: Reg::Phys(phys),
                offset,
            });
        }
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(phys),
                    offset,
                });
            } else {
                let slot = alloc
                    .stack_offset
                    .ok_or_else(|| AllocError::new("spilled vreg is missing a stack slot"))?;
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(PhysReg::R10),
                    offset,
                });
                out.push(MirInst::StoreStack {
                    src: Reg::Phys(PhysReg::R10),
                    offset: slot,
                });
            }
        }
    }

    Ok(out)
}

fn lower_store_stack_x86_64(
    src: Reg,
    offset: i32,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();

    match src {
        Reg::Phys(phys) => {
            out.push(MirInst::StoreStack {
                src: Reg::Phys(phys),
                offset,
            });
        }
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                out.push(MirInst::StoreStack {
                    src: Reg::Phys(phys),
                    offset,
                });
            } else {
                let slot = alloc
                    .stack_offset
                    .ok_or_else(|| AllocError::new("spilled vreg is missing a stack slot"))?;
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(PhysReg::R10),
                    offset: slot,
                });
                out.push(MirInst::StoreStack {
                    src: Reg::Phys(PhysReg::R10),
                    offset,
                });
            }
        }
    }

    Ok(out)
}

fn lower_arithmetic_x86_64(
    op: ArithOp,
    dst: Reg,
    lhs: Reg,
    rhs: &Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let forbidden = collect_phys_regs_for_arith(dst, lhs, rhs);
    let mut used_scratch = Vec::new();
    let mut out = Vec::new();

    let resolved_lhs =
        resolve_use_reg_x86_64(lhs, allocations, &mut out, &mut used_scratch, &forbidden)?;
    let resolved_rhs = resolve_operand_use_x86_64(
        rhs.clone(),
        allocations,
        &mut out,
        &mut used_scratch,
        &forbidden,
    )?;

    let rhs_phys = if let Operand::Reg(Reg::Phys(phys)) = resolved_rhs.operand {
        Some(phys)
    } else {
        None
    };

    let (dst_reg, spill_offset) = match dst {
        Reg::Phys(phys) => (Reg::Phys(phys), None),
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                (Reg::Phys(phys), None)
            } else {
                let offset = alloc
                    .stack_offset
                    .ok_or_else(|| AllocError::new("spilled vreg is missing a stack slot"))?;

                let reuses_lhs_value = matches!(lhs, Reg::VReg(lhs_vreg) if lhs_vreg == vreg);
                let dst_phys = if reuses_lhs_value || resolved_lhs.is_temp {
                    match resolved_lhs.reg {
                        Reg::Phys(phys) => phys,
                        Reg::VReg(_) => {
                            return Err(AllocError::new("resolved lhs still contains virtual reg"));
                        }
                    }
                } else {
                    let mut blocked = forbidden.clone();
                    if let Reg::Phys(lhs_phys) = resolved_lhs.reg {
                        blocked.push(lhs_phys);
                    }
                    if let Some(rhs_phys) = rhs_phys {
                        blocked.push(rhs_phys);
                    }
                    alloc_scratch(&mut used_scratch, &blocked)?
                };

                (Reg::Phys(dst_phys), Some(offset))
            }
        }
    };

    let arith_inst = match op {
        ArithOp::Add => MirInst::Add {
            dst: dst_reg,
            lhs: resolved_lhs.reg,
            rhs: resolved_rhs.operand,
        },
        ArithOp::Sub => MirInst::Sub {
            dst: dst_reg,
            lhs: resolved_lhs.reg,
            rhs: resolved_rhs.operand,
        },
        ArithOp::And => MirInst::And {
            dst: dst_reg,
            lhs: resolved_lhs.reg,
            rhs: resolved_rhs.operand,
        },
        ArithOp::Mul => MirInst::Mul {
            dst: dst_reg,
            lhs: resolved_lhs.reg,
            rhs: resolved_rhs.operand,
        },
        ArithOp::Sdiv => MirInst::Sdiv {
            dst: dst_reg,
            lhs: resolved_lhs.reg,
            rhs: resolved_rhs.operand,
        },
    };
    out.push(arith_inst);

    if let Some(offset) = spill_offset {
        out.push(MirInst::StoreStack {
            src: dst_reg,
            offset,
        });
    }

    Ok(out)
}

fn lower_cmp_x86_64(
    lhs: Reg,
    rhs: &Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<Vec<MirInst>, AllocError> {
    let forbidden = collect_phys_regs_for_cmp(lhs, rhs);
    let mut used_scratch = Vec::new();
    let mut out = Vec::new();

    let resolved_lhs =
        resolve_use_reg_x86_64(lhs, allocations, &mut out, &mut used_scratch, &forbidden)?;
    let resolved_rhs = resolve_operand_use_x86_64(
        rhs.clone(),
        allocations,
        &mut out,
        &mut used_scratch,
        &forbidden,
    )?;

    out.push(MirInst::Cmp {
        lhs: resolved_lhs.reg,
        rhs: resolved_rhs.operand,
    });

    Ok(out)
}

fn resolve_use_reg_x86_64(
    reg: Reg,
    allocations: &HashMap<usize, WorkingAlloc>,
    out: &mut Vec<MirInst>,
    used_scratch: &mut Vec<PhysReg>,
    forbidden: &[PhysReg],
) -> Result<ResolvedReg, AllocError> {
    match reg {
        Reg::Phys(phys) => Ok(ResolvedReg {
            reg: Reg::Phys(phys),
            is_temp: false,
        }),
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                Ok(ResolvedReg {
                    reg: Reg::Phys(phys),
                    is_temp: false,
                })
            } else {
                let offset = alloc
                    .stack_offset
                    .ok_or_else(|| AllocError::new("spilled vreg is missing a stack slot"))?;
                let scratch = alloc_scratch(used_scratch, forbidden)?;
                out.push(MirInst::LoadStack {
                    dst: Reg::Phys(scratch),
                    offset,
                });
                Ok(ResolvedReg {
                    reg: Reg::Phys(scratch),
                    is_temp: true,
                })
            }
        }
    }
}

fn resolve_operand_use_x86_64(
    operand: Operand,
    allocations: &HashMap<usize, WorkingAlloc>,
    out: &mut Vec<MirInst>,
    used_scratch: &mut Vec<PhysReg>,
    forbidden: &[PhysReg],
) -> Result<ResolvedOperand, AllocError> {
    match operand {
        Operand::Imm(imm) => Ok(ResolvedOperand {
            operand: Operand::Imm(imm),
        }),
        Operand::Reg(reg) => {
            let resolved = resolve_use_reg_x86_64(reg, allocations, out, used_scratch, forbidden)?;
            Ok(ResolvedOperand {
                operand: Operand::Reg(resolved.reg),
            })
        }
    }
}

fn collect_vreg_uses(inst: &MirInst, out: &mut Vec<usize>) {
    match inst {
        MirInst::Mov { src, .. } => push_operand_vreg(src, out),
        MirInst::Add { lhs, rhs, .. }
        | MirInst::Sub { lhs, rhs, .. }
        | MirInst::And { lhs, rhs, .. }
        | MirInst::Mul { lhs, rhs, .. } => {
            push_reg_vreg(lhs, out);
            push_operand_vreg(rhs, out);
        }
        MirInst::Sdiv { lhs, rhs, .. } => {
            push_reg_vreg(lhs, out);
            push_operand_vreg(rhs, out);
        }
        MirInst::Cmp { lhs, rhs } => {
            push_reg_vreg(lhs, out);
            push_operand_vreg(rhs, out);
        }
        MirInst::StoreStack { src, .. } | MirInst::Push { src } => {
            push_reg_vreg(src, out);
        }
        MirInst::Label(_)
        | MirInst::Jmp { .. }
        | MirInst::JmpIf { .. }
        | MirInst::Call { .. }
        | MirInst::Ret
        | MirInst::LoadStack { .. }
        | MirInst::Pop { .. } => {}
    }
}

fn collect_vreg_defs(inst: &MirInst, out: &mut Vec<usize>) {
    match inst {
        MirInst::Mov { dst, .. }
        | MirInst::Add { dst, .. }
        | MirInst::Sub { dst, .. }
        | MirInst::And { dst, .. }
        | MirInst::Mul { dst, .. }
        | MirInst::Sdiv { dst, .. }
        | MirInst::LoadStack { dst, .. }
        | MirInst::Pop { dst } => {
            push_reg_vreg(dst, out);
        }
        MirInst::Label(_)
        | MirInst::Cmp { .. }
        | MirInst::Jmp { .. }
        | MirInst::JmpIf { .. }
        | MirInst::Call { .. }
        | MirInst::Ret
        | MirInst::StoreStack { .. }
        | MirInst::Push { .. } => {}
    }
}

fn push_reg_vreg(reg: &Reg, out: &mut Vec<usize>) {
    if let Reg::VReg(vreg) = reg {
        out.push(*vreg);
    }
}

fn push_operand_vreg(operand: &Operand, out: &mut Vec<usize>) {
    if let Operand::Reg(reg) = operand {
        push_reg_vreg(reg, out);
    }
}

fn contains_vreg(reg: Reg) -> bool {
    matches!(reg, Reg::VReg(_))
}

fn expire_old_intervals(
    current_start: usize,
    active: &mut Vec<ActiveInterval>,
    free_regs: &mut Vec<PhysReg>,
    allocatable: &[PhysReg],
) {
    let mut index = 0;
    while index < active.len() {
        if active[index].end < current_start {
            let expired = active.remove(index);
            free_regs.push(expired.reg);
        } else {
            index += 1;
        }
    }

    free_regs.sort_by_key(|reg| {
        allocatable
            .iter()
            .position(|candidate| candidate == reg)
            .unwrap_or(usize::MAX)
    });
}

fn ensure_stack_slot(
    vreg: usize,
    working: &mut HashMap<usize, WorkingAlloc>,
    next_stack_offset: &mut i32,
) -> i32 {
    let state = working.entry(vreg).or_default();
    if let Some(offset) = state.stack_offset {
        offset
    } else {
        *next_stack_offset += 8;
        state.stack_offset = Some(*next_stack_offset);
        *next_stack_offset
    }
}

fn max_preexisting_stack_offset(func: &MirFunction) -> i32 {
    let mut max_offset = 0;
    for inst in &func.instructions {
        match inst {
            MirInst::LoadStack { offset, .. } | MirInst::StoreStack { offset, .. } => {
                max_offset = max_offset.max(*offset);
            }
            _ => {}
        }
    }
    max_offset
}

fn alloc_scratch(
    used_scratch: &mut Vec<PhysReg>,
    forbidden: &[PhysReg],
) -> Result<PhysReg, AllocError> {
    for scratch in [PhysReg::R10, PhysReg::R11] {
        if used_scratch.contains(&scratch) || forbidden.contains(&scratch) {
            continue;
        }
        used_scratch.push(scratch);
        return Ok(scratch);
    }

    Err(AllocError::new(
        "ran out of scratch registers while materializing spills",
    ))
}

fn get_alloc(
    vreg: usize,
    allocations: &HashMap<usize, WorkingAlloc>,
) -> Result<WorkingAlloc, AllocError> {
    allocations
        .get(&vreg)
        .copied()
        .ok_or_else(|| AllocError::new(format!("missing allocation for v{vreg}")))
}

fn align_stack_size_for_calls(raw_stack_size: usize, callee_saved_count: usize) -> usize {
    let mut stack_size = raw_stack_size;
    let push_alignment = if callee_saved_count % 2 == 0 { 0 } else { 8 };
    let remainder = stack_size % 16;
    if remainder != push_alignment {
        stack_size += (push_alignment + 16 - remainder) % 16;
    }
    stack_size
}

fn collect_used_callee_saved_registers(working: &HashMap<usize, WorkingAlloc>) -> Vec<PhysReg> {
    let mut regs = Vec::new();
    for state in working.values() {
        if let Some(reg) = state.reg
            && is_x86_64_callee_saved(reg)
            && !regs.contains(&reg)
        {
            regs.push(reg);
        }
    }
    regs.sort_by_key(|reg| match reg {
        PhysReg::RBX => 0,
        PhysReg::R12 => 1,
        PhysReg::R13 => 2,
        PhysReg::R14 => 3,
        PhysReg::R15 => 4,
        _ => usize::MAX,
    });
    regs
}

fn collect_phys_regs_for_mov(dst: Reg, src: &Operand) -> Vec<PhysReg> {
    let mut regs = Vec::new();
    if let Reg::Phys(phys) = dst {
        regs.push(phys);
    }
    if let Operand::Reg(Reg::Phys(phys)) = src {
        regs.push(*phys);
    }
    regs
}

fn collect_phys_regs_for_arith(dst: Reg, lhs: Reg, rhs: &Operand) -> Vec<PhysReg> {
    let mut regs = Vec::new();
    if let Reg::Phys(phys) = dst {
        regs.push(phys);
    }
    if let Reg::Phys(phys) = lhs {
        regs.push(phys);
    }
    if let Operand::Reg(Reg::Phys(phys)) = rhs {
        regs.push(*phys);
    }
    regs
}

fn collect_phys_regs_for_cmp(lhs: Reg, rhs: &Operand) -> Vec<PhysReg> {
    let mut regs = Vec::new();
    if let Reg::Phys(phys) = lhs {
        regs.push(phys);
    }
    if let Operand::Reg(Reg::Phys(phys)) = rhs {
        regs.push(*phys);
    }
    regs
}

fn allocatable_arm64_registers() -> &'static [PhysReg] {
    // Include argument registers to maximize coalescing on leaf functions,
    // but keep scratch temporaries/reserved ABI registers out of allocation.
    &[
        PhysReg::X0,
        PhysReg::X1,
        PhysReg::X2,
        PhysReg::X3,
        PhysReg::X4,
        PhysReg::X5,
        PhysReg::X6,
        PhysReg::X7,
        PhysReg::X9,
        PhysReg::X10,
        PhysReg::X11,
        PhysReg::X12,
        PhysReg::X13,
        PhysReg::X14,
    ]
}

fn is_arm64_caller_saved(reg: PhysReg) -> bool {
    matches!(
        reg,
        PhysReg::X0
            | PhysReg::X1
            | PhysReg::X2
            | PhysReg::X3
            | PhysReg::X4
            | PhysReg::X5
            | PhysReg::X6
            | PhysReg::X7
            | PhysReg::X8
            | PhysReg::X9
            | PhysReg::X10
            | PhysReg::X11
            | PhysReg::X12
            | PhysReg::X13
            | PhysReg::X14
            | PhysReg::X15
            | PhysReg::X16
            | PhysReg::X17
    )
}

fn allocatable_x86_64_registers() -> &'static [PhysReg] {
    &[
        PhysReg::RCX,
        PhysReg::RSI,
        PhysReg::RDI,
        PhysReg::R8,
        PhysReg::R9,
        PhysReg::RBX,
        PhysReg::R12,
        PhysReg::R13,
        PhysReg::R14,
        PhysReg::R15,
    ]
}

fn is_x86_64_caller_saved(reg: PhysReg) -> bool {
    matches!(
        reg,
        PhysReg::RAX
            | PhysReg::RCX
            | PhysReg::RDX
            | PhysReg::RSI
            | PhysReg::RDI
            | PhysReg::R8
            | PhysReg::R9
            | PhysReg::R10
            | PhysReg::R11
    )
}

fn is_x86_64_callee_saved(reg: PhysReg) -> bool {
    matches!(
        reg,
        PhysReg::RBX | PhysReg::R12 | PhysReg::R13 | PhysReg::R14 | PhysReg::R15
    )
}

#[derive(Debug, Clone, Copy)]
enum ArithOp {
    Add,
    Sub,
    And,
    Mul,
    Sdiv,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mir::emit_x86_64_assembly;

    #[test]
    fn computes_simple_live_intervals() {
        let func = MirFunction::with_instructions(
            "intervals",
            vec![
                MirInst::Mov {
                    dst: Reg::VReg(0),
                    src: Operand::Imm(1),
                },
                MirInst::Mov {
                    dst: Reg::VReg(1),
                    src: Operand::Imm(2),
                },
                MirInst::Add {
                    dst: Reg::VReg(2),
                    lhs: Reg::VReg(0),
                    rhs: Operand::Reg(Reg::VReg(1)),
                },
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RAX),
                    src: Operand::Reg(Reg::VReg(2)),
                },
                MirInst::Ret,
            ],
        );

        let intervals = compute_live_intervals(&func);
        assert_eq!(intervals.len(), 3);
        assert_eq!(
            intervals[0],
            LiveInterval {
                vreg: 0,
                start: 0,
                end: 2
            }
        );
        assert_eq!(
            intervals[1],
            LiveInterval {
                vreg: 1,
                start: 1,
                end: 2
            }
        );
        assert_eq!(
            intervals[2],
            LiveInterval {
                vreg: 2,
                start: 2,
                end: 3
            }
        );
    }

    #[test]
    fn linear_scan_spills_under_pressure() {
        let mut insts = Vec::new();
        for vreg in 0..30 {
            insts.push(MirInst::Mov {
                dst: Reg::VReg(vreg),
                src: Operand::Imm(vreg as i64),
            });
        }

        insts.push(MirInst::Mov {
            dst: Reg::VReg(100),
            src: Operand::Imm(0),
        });
        for vreg in 0..30 {
            insts.push(MirInst::Add {
                dst: Reg::VReg(100),
                lhs: Reg::VReg(100),
                rhs: Operand::Reg(Reg::VReg(vreg)),
            });
        }

        insts.push(MirInst::Mov {
            dst: Reg::Phys(PhysReg::RAX),
            src: Operand::Reg(Reg::VReg(100)),
        });
        insts.push(MirInst::Ret);

        let func = MirFunction::with_instructions("spill_pressure", insts);
        let allocation =
            linear_scan_allocate(&func, TargetArch::X86_64).expect("linear scan should succeed");

        assert!(
            allocation.stack_size > 0,
            "expected spills to allocate stack space"
        );

        let asm = emit_x86_64_assembly(&allocation.function).expect("x86_64 emission failed");
        assert!(
            asm.contains("(%rbp)"),
            "expected stack slot references in assembly"
        );
    }

    #[test]
    fn linear_scan_evicts_caller_saved_around_call() {
        let func = MirFunction::with_instructions(
            "call_clobber",
            vec![
                MirInst::Mov {
                    dst: Reg::VReg(0),
                    src: Operand::Imm(7),
                },
                MirInst::Mov {
                    dst: Reg::VReg(1),
                    src: Operand::Imm(3),
                },
                MirInst::Add {
                    dst: Reg::VReg(2),
                    lhs: Reg::VReg(0),
                    rhs: Operand::Reg(Reg::VReg(1)),
                },
                MirInst::Call {
                    symbol: "helper".to_string(),
                },
                MirInst::Add {
                    dst: Reg::VReg(3),
                    lhs: Reg::VReg(2),
                    rhs: Operand::Imm(1),
                },
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RAX),
                    src: Operand::Reg(Reg::VReg(3)),
                },
                MirInst::Ret,
            ],
        );

        let allocation =
            linear_scan_allocate(&func, TargetArch::X86_64).expect("linear scan should succeed");

        let mut saw_store_before_call = false;
        let mut saw_load_after_call = false;
        for window in allocation.function.instructions.windows(3) {
            if matches!(&window[1], MirInst::Call { .. }) {
                saw_store_before_call = matches!(&window[0], MirInst::StoreStack { .. });
                saw_load_after_call = matches!(&window[2], MirInst::LoadStack { .. });
                break;
            }
        }

        assert!(
            saw_store_before_call,
            "expected caller-saved register eviction before call"
        );
        assert!(
            saw_load_after_call,
            "expected caller-saved register restore after call"
        );
    }

    #[test]
    fn linear_scan_supports_amd64_alias() {
        let func = MirFunction::with_instructions(
            "amd64_alias",
            vec![
                MirInst::Mov {
                    dst: Reg::VReg(0),
                    src: Operand::Imm(5),
                },
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RAX),
                    src: Operand::Reg(Reg::VReg(0)),
                },
                MirInst::Ret,
            ],
        );

        let allocation =
            linear_scan_allocate(&func, TargetArch::Amd64).expect("amd64 allocation should work");
        let asm = emit_x86_64_assembly(&allocation.function).expect("x86_64 emission failed");

        assert!(asm.contains("movq $5"));
        assert!(asm.contains("ret"));
    }

    #[test]
    fn preserves_used_callee_saved_registers() {
        let func = MirFunction::with_instructions(
            "callee_saved_preserve",
            vec![
                MirInst::Mov {
                    dst: Reg::VReg(0),
                    src: Operand::Imm(1),
                },
                MirInst::Mov {
                    dst: Reg::VReg(1),
                    src: Operand::Imm(2),
                },
                MirInst::Mov {
                    dst: Reg::VReg(2),
                    src: Operand::Imm(3),
                },
                MirInst::Mov {
                    dst: Reg::VReg(3),
                    src: Operand::Imm(4),
                },
                MirInst::Mov {
                    dst: Reg::VReg(4),
                    src: Operand::Imm(5),
                },
                MirInst::Mov {
                    dst: Reg::VReg(5),
                    src: Operand::Imm(6),
                },
                MirInst::Add {
                    dst: Reg::VReg(6),
                    lhs: Reg::VReg(0),
                    rhs: Operand::Reg(Reg::VReg(1)),
                },
                MirInst::Add {
                    dst: Reg::VReg(7),
                    lhs: Reg::VReg(2),
                    rhs: Operand::Reg(Reg::VReg(3)),
                },
                MirInst::Add {
                    dst: Reg::VReg(8),
                    lhs: Reg::VReg(4),
                    rhs: Operand::Reg(Reg::VReg(5)),
                },
                MirInst::Add {
                    dst: Reg::VReg(9),
                    lhs: Reg::VReg(6),
                    rhs: Operand::Reg(Reg::VReg(7)),
                },
                MirInst::Add {
                    dst: Reg::VReg(10),
                    lhs: Reg::VReg(9),
                    rhs: Operand::Reg(Reg::VReg(8)),
                },
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RAX),
                    src: Operand::Reg(Reg::VReg(10)),
                },
                MirInst::Ret,
            ],
        );

        let allocation =
            linear_scan_allocate(&func, TargetArch::Amd64).expect("linear scan should succeed");

        let mut used_callee_saved = Vec::new();
        for state in allocation.allocations.values() {
            if let Some(reg) = state.reg
                && is_x86_64_callee_saved(reg)
                && !used_callee_saved.contains(&reg)
            {
                used_callee_saved.push(reg);
            }
        }

        assert!(
            !used_callee_saved.is_empty(),
            "expected at least one callee-saved register to be allocated"
        );

        for reg in used_callee_saved {
            assert!(allocation.function.instructions.iter().any(|inst| {
                matches!(
                    inst,
                    MirInst::Push {
                        src: Reg::Phys(pushed),
                    } if *pushed == reg
                )
            }));
            assert!(allocation.function.instructions.iter().any(|inst| {
                matches!(
                    inst,
                    MirInst::Pop {
                        dst: Reg::Phys(popped),
                    } if *popped == reg
                )
            }));
        }
    }
}
