pub mod ir;
pub mod mir;
pub mod regalloc;

pub use ir::{
    BasicBlock, BlockId, Function, FunctionParam, IcmpPredicate, InstrId, Instruction,
    IrBuildError, IrBuilder, PhiIncoming, Type, ValueData, ValueId, ValueKind, build_factorial_il,
};
pub use mir::{
    Cond, EmitError, MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch, emit_arm64_assembly,
    emit_assembly, emit_x86_64_assembly,
};
pub use regalloc::{
    AllocError, LinearScanAllocation, LiveInterval, VRegAllocation, compute_live_intervals,
    linear_scan_allocate,
};
