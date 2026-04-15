pub mod ir;
pub mod mir;

pub use ir::{BlockId, InstrId, Type, ValueId};
pub use mir::{
    Cond, EmitError, MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch, emit_arm64_assembly,
    emit_assembly, emit_x86_64_assembly,
};
