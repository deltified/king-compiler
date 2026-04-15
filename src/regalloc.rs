use std::collections::{BTreeMap, HashMap};
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
    let mut ranges: HashMap<usize, (usize, usize)> = HashMap::new();

    for (index, inst) in func.instructions.iter().enumerate() {
        let mut uses = Vec::new();
        let mut defs = Vec::new();
        collect_vreg_uses(inst, &mut uses);
        collect_vreg_defs(inst, &mut defs);

        for vreg in uses.into_iter().chain(defs) {
            let entry = ranges.entry(vreg).or_insert((index, index));
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

pub fn linear_scan_allocate(
    func: &MirFunction,
    target: TargetArch,
) -> Result<LinearScanAllocation, AllocError> {
    match target {
        TargetArch::Amd64 => linear_scan_allocate_x86_64(func),
        TargetArch::X86_64 => linear_scan_allocate_x86_64(func),
        TargetArch::Arm64 => Err(AllocError::new(
            "linear-scan allocation is currently implemented for amd64/x86_64",
        )),
    }
}

fn linear_scan_allocate_x86_64(func: &MirFunction) -> Result<LinearScanAllocation, AllocError> {
    let intervals = compute_live_intervals(func);
    let allocatable = allocatable_x86_64_registers();

    let mut working: HashMap<usize, WorkingAlloc> = HashMap::new();
    let mut active: Vec<ActiveInterval> = Vec::new();
    let mut free_regs = allocatable.to_vec();
    let mut next_stack_offset: i32 = 0;

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

            let state = working
                .get(&interval.vreg)
                .ok_or_else(|| AllocError::new("interval has no allocation state"))?;
            if let Some(reg) = state.reg
                && is_x86_64_caller_saved(reg)
            {
                ensure_stack_slot(interval.vreg, &mut working, &mut next_stack_offset);
            }
        }
    }

    let raw_stack_size = next_stack_offset.max(0) as usize;
    let stack_size = align_to_16(raw_stack_size);

    let lowered = rewrite_with_allocations_x86_64(func, &intervals, &working, stack_size)?;
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
) -> Result<Vec<MirInst>, AllocError> {
    let mut out = Vec::new();

    out.push(MirInst::Push {
        src: Reg::Phys(PhysReg::RBP),
    });
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

    for (index, inst) in func.instructions.iter().enumerate() {
        match inst {
            MirInst::Label(label) => out.push(MirInst::Label(label.clone())),
            MirInst::Mov { dst, src } => {
                out.extend(lower_mov_x86_64(*dst, src, allocations)?);
            }
            MirInst::Add { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_x86_64(true, *dst, *lhs, rhs, allocations)?);
            }
            MirInst::Sub { dst, lhs, rhs } => {
                out.extend(lower_arithmetic_x86_64(
                    false,
                    *dst,
                    *lhs,
                    rhs,
                    allocations,
                )?);
            }
            MirInst::Cmp { lhs, rhs } => {
                out.extend(lower_cmp_x86_64(*lhs, rhs, allocations)?);
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
                out.push(MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RSP),
                    src: Operand::Reg(Reg::Phys(PhysReg::RBP)),
                });
                out.push(MirInst::Pop {
                    dst: Reg::Phys(PhysReg::RBP),
                });
                out.push(MirInst::Ret);
            }
            MirInst::Push { src }
            | MirInst::Pop { dst: src }
            | MirInst::LoadStack { dst: src, .. }
            | MirInst::StoreStack { src, .. } => {
                if contains_vreg(*src) {
                    return Err(AllocError::new(
                        "input MIR must not contain stack/push/pop ops with virtual registers",
                    ));
                }
                out.push(inst.clone());
            }
        }
    }

    Ok(out)
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
            out.push(MirInst::Mov {
                dst: Reg::Phys(phys),
                src: resolved_src.operand,
            });
        }
        Reg::VReg(vreg) => {
            let alloc = get_alloc(vreg, allocations)?;
            if let Some(phys) = alloc.reg {
                out.push(MirInst::Mov {
                    dst: Reg::Phys(phys),
                    src: resolved_src.operand,
                });
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

fn lower_arithmetic_x86_64(
    is_add: bool,
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

    let arith_inst = if is_add {
        MirInst::Add {
            dst: dst_reg,
            lhs: resolved_lhs.reg,
            rhs: resolved_rhs.operand,
        }
    } else {
        MirInst::Sub {
            dst: dst_reg,
            lhs: resolved_lhs.reg,
            rhs: resolved_rhs.operand,
        }
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
        MirInst::Add { lhs, rhs, .. } | MirInst::Sub { lhs, rhs, .. } => {
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

fn align_to_16(value: usize) -> usize {
    if value == 0 { 0 } else { (value + 15) & !15 }
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

fn allocatable_x86_64_registers() -> &'static [PhysReg] {
    &[
        PhysReg::RAX,
        PhysReg::RCX,
        PhysReg::RDX,
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

        assert!(asm.contains("pushq %rbp"));
        assert!(asm.contains("ret"));
    }
}
