pub mod ir;
pub mod lowering;
pub mod mir;
pub mod regalloc;

pub use ir::{
    BasicBlock, BlockId, Function, FunctionParam, IcmpPredicate, InstrId, Instruction,
    IrBuildError, IrBuilder, PhiIncoming, Type, ValueData, ValueId, ValueKind, build_factorial_il,
    constant_fold, dead_code_elimination, run_phase5_pipeline, simplify_cfg,
};
pub use lowering::{LoweringError, PhiCopy, PhiElimination, eliminate_phi_nodes, lower_il_to_mir};
pub use mir::{
    Cond, EmitError, MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch, emit_arm64_assembly,
    emit_assembly, emit_x86_64_assembly,
};
pub use regalloc::{
    AllocError, LinearScanAllocation, LiveInterval, VRegAllocation, compute_live_intervals,
    linear_scan_allocate,
};
