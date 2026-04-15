pub mod ir;
pub mod mir;
pub mod regalloc;

pub use ir::{BlockId, InstrId, Type, ValueId};
pub use mir::{
    Cond, EmitError, MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch, emit_arm64_assembly,
    emit_assembly, emit_x86_64_assembly,
};
pub use regalloc::{
    AllocError, LinearScanAllocation, LiveInterval, VRegAllocation, compute_live_intervals,
    linear_scan_allocate,
};
