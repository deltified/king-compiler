use slotmap::SlotMap;

slotmap::new_key_type! {
    pub struct InstrId;
}

slotmap::new_key_type! {
    pub struct BlockId;
}

slotmap::new_key_type! {
    pub struct ValueId;
}

pub type InstrArena<T> = SlotMap<InstrId, T>;
pub type BlockArena<T> = SlotMap<BlockId, T>;
pub type ValueArena<T> = SlotMap<ValueId, T>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    I8,
    I32,
    I64,
    Ptr,
    Void,
}