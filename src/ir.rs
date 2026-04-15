use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};

use petgraph::graph::{DiGraph, NodeIndex};
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

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::I8 => "i8",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::Ptr => "ptr",
            Self::Void => "void",
        };
        write!(f, "{text}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IcmpPredicate {
    Eq,
    Ne,
    Slt,
    Sle,
    Sgt,
    Sge,
}

impl Display for IcmpPredicate {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::Slt => "slt",
            Self::Sle => "sle",
            Self::Sgt => "sgt",
            Self::Sge => "sge",
        };
        write!(f, "{text}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhiIncoming {
    pub value: ValueId,
    pub block: BlockId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Add {
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },
    Sub {
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },
    Mul {
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },
    Sdiv {
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },
    And {
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },
    Alloca {
        ty: Type,
    },
    Store {
        ty: Type,
        value: ValueId,
        ptr: ValueId,
    },
    Load {
        ty: Type,
        ptr: ValueId,
    },
    Icmp {
        pred: IcmpPredicate,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    },
    Call {
        ret_ty: Type,
        function: String,
        args: Vec<(Type, ValueId)>,
    },
    Phi {
        ty: Type,
        incomings: Vec<PhiIncoming>,
    },
    Jmp {
        target: BlockId,
    },
    Br {
        cond: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
    Ret {
        value: Option<ValueId>,
    },
}

impl Instruction {
    pub fn has_side_effects(&self) -> bool {
        matches!(
            self,
            Self::Store { .. }
                | Self::Call { .. }
                | Self::Jmp { .. }
                | Self::Br { .. }
                | Self::Ret { .. }
        )
    }

    pub fn result_type(&self) -> Option<Type> {
        match self {
            Self::Add { ty, .. }
            | Self::Sub { ty, .. }
            | Self::Mul { ty, .. }
            | Self::Sdiv { ty, .. }
            | Self::And { ty, .. }
            | Self::Load { ty, .. }
            | Self::Phi { ty, .. } => Some(*ty),
            Self::Alloca { .. } => Some(Type::Ptr),
            Self::Icmp { .. } => Some(Type::I8),
            Self::Call { ret_ty, .. } => {
                if *ret_ty == Type::Void {
                    None
                } else {
                    Some(*ret_ty)
                }
            }
            Self::Store { .. } | Self::Jmp { .. } | Self::Br { .. } | Self::Ret { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicBlock {
    pub name: String,
    pub instructions: Vec<InstrId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParam {
    pub name: String,
    pub ty: Type,
    pub value: ValueId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueKind {
    ConstantInt(i64),
    Parameter(String),
    InstructionResult(InstrId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueData {
    pub ty: Type,
    pub kind: ValueKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrBuildError {
    MissingCurrentBlock,
    UnknownBlock(BlockId),
    UnknownValue(ValueId),
    TypeMismatch {
        context: &'static str,
        expected: Type,
        found: Type,
    },
    InvalidType {
        context: &'static str,
        ty: Type,
    },
    NotAPhi(ValueId),
    ValueDoesNotComeFromInstruction(ValueId),
}

impl Display for IrBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCurrentBlock => write!(f, "IR builder has no current block"),
            Self::UnknownBlock(block) => write!(f, "unknown block: {block:?}"),
            Self::UnknownValue(value) => write!(f, "unknown value: {value:?}"),
            Self::TypeMismatch {
                context,
                expected,
                found,
            } => {
                write!(
                    f,
                    "type mismatch in {context}: expected {expected}, found {found}"
                )
            }
            Self::InvalidType { context, ty } => {
                write!(f, "invalid type in {context}: {ty}")
            }
            Self::NotAPhi(value) => write!(f, "value {value:?} is not produced by a phi"),
            Self::ValueDoesNotComeFromInstruction(value) => write!(
                f,
                "value {value:?} does not come from an instruction result"
            ),
        }
    }
}

impl Error for IrBuildError {}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub return_type: Type,
    pub params: Vec<FunctionParam>,
    pub values: ValueArena<ValueData>,
    pub instructions: InstrArena<Instruction>,
    pub blocks: BlockArena<BasicBlock>,
    pub cfg: DiGraph<BlockId, ()>,
    block_nodes: HashMap<BlockId, NodeIndex>,
    block_order: Vec<BlockId>,
    instr_results: HashMap<InstrId, ValueId>,
}

impl Function {
    pub fn new(name: impl Into<String>, return_type: Type) -> Self {
        Self {
            name: name.into(),
            return_type,
            params: Vec::new(),
            values: ValueArena::with_key(),
            instructions: InstrArena::with_key(),
            blocks: BlockArena::with_key(),
            cfg: DiGraph::new(),
            block_nodes: HashMap::new(),
            block_order: Vec::new(),
            instr_results: HashMap::new(),
        }
    }

    pub fn add_param(&mut self, name: impl Into<String>, ty: Type) -> ValueId {
        let name = name.into();
        let value = self.values.insert(ValueData {
            ty,
            kind: ValueKind::Parameter(name.clone()),
        });

        self.params.push(FunctionParam { name, ty, value });
        value
    }

    pub fn create_block(&mut self, name: impl Into<String>) -> BlockId {
        let block_id = self.blocks.insert(BasicBlock {
            name: name.into(),
            instructions: Vec::new(),
        });

        let node = self.cfg.add_node(block_id);
        self.block_nodes.insert(block_id, node);
        self.block_order.push(block_id);
        block_id
    }

    pub fn add_edge(&mut self, from: BlockId, to: BlockId) -> Result<(), IrBuildError> {
        let from_node = self
            .block_nodes
            .get(&from)
            .copied()
            .ok_or(IrBuildError::UnknownBlock(from))?;
        let to_node = self
            .block_nodes
            .get(&to)
            .copied()
            .ok_or(IrBuildError::UnknownBlock(to))?;

        if self.cfg.find_edge(from_node, to_node).is_none() {
            self.cfg.add_edge(from_node, to_node, ());
        }

        Ok(())
    }

    pub fn append_instruction(
        &mut self,
        block: BlockId,
        instruction: Instruction,
    ) -> Result<Option<ValueId>, IrBuildError> {
        self.apply_cfg_edges(block, &instruction)?;

        let result_ty = instruction.result_type();
        let instr_id = self.instructions.insert(instruction);

        let block_data = self
            .blocks
            .get_mut(block)
            .ok_or(IrBuildError::UnknownBlock(block))?;
        block_data.instructions.push(instr_id);

        if let Some(ty) = result_ty {
            let value = self.values.insert(ValueData {
                ty,
                kind: ValueKind::InstructionResult(instr_id),
            });
            self.instr_results.insert(instr_id, value);
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn apply_cfg_edges(
        &mut self,
        block: BlockId,
        instruction: &Instruction,
    ) -> Result<(), IrBuildError> {
        match instruction {
            Instruction::Jmp { target } => self.add_edge(block, *target),
            Instruction::Br {
                then_block,
                else_block,
                ..
            } => {
                self.add_edge(block, *then_block)?;
                self.add_edge(block, *else_block)
            }
            _ => Ok(()),
        }
    }

    pub fn value_type(&self, value: ValueId) -> Option<Type> {
        self.values.get(value).map(|data| data.ty)
    }

    pub fn value(&self, value: ValueId) -> Option<&ValueData> {
        self.values.get(value)
    }

    pub fn instruction(&self, instruction: InstrId) -> Option<&Instruction> {
        self.instructions.get(instruction)
    }

    fn block_label(&self, block: BlockId) -> String {
        self.blocks
            .get(block)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "missing".to_string())
    }

    fn value_text(&self, value: ValueId, value_order: &HashMap<ValueId, usize>) -> String {
        let Some(data) = self.values.get(value) else {
            return "%<missing>".to_string();
        };

        match &data.kind {
            ValueKind::ConstantInt(number) => number.to_string(),
            ValueKind::Parameter(name) => format!("%{name}"),
            ValueKind::InstructionResult(_) => {
                let index = value_order.get(&value).copied().unwrap_or(0);
                format!("%{index}")
            }
        }
    }

    fn typed_value_text(&self, value: ValueId, value_order: &HashMap<ValueId, usize>) -> String {
        let ty = self.value_type(value).unwrap_or(Type::Void);
        format!("{ty} {}", self.value_text(value, value_order))
    }

    fn format_instruction(
        &self,
        instruction: &Instruction,
        instr_id: InstrId,
        value_order: &HashMap<ValueId, usize>,
    ) -> String {
        let prefix = self
            .instr_results
            .get(&instr_id)
            .map(|value| format!("{} = ", self.value_text(*value, value_order)))
            .unwrap_or_default();

        match instruction {
            Instruction::Add { ty, lhs, rhs } => {
                format!(
                    "{prefix}add {ty} {}, {}",
                    self.value_text(*lhs, value_order),
                    self.value_text(*rhs, value_order)
                )
            }
            Instruction::Sub { ty, lhs, rhs } => {
                format!(
                    "{prefix}sub {ty} {}, {}",
                    self.value_text(*lhs, value_order),
                    self.value_text(*rhs, value_order)
                )
            }
            Instruction::Mul { ty, lhs, rhs } => {
                format!(
                    "{prefix}mul {ty} {}, {}",
                    self.value_text(*lhs, value_order),
                    self.value_text(*rhs, value_order)
                )
            }
            Instruction::Sdiv { ty, lhs, rhs } => {
                format!(
                    "{prefix}sdiv {ty} {}, {}",
                    self.value_text(*lhs, value_order),
                    self.value_text(*rhs, value_order)
                )
            }
            Instruction::And { ty, lhs, rhs } => {
                format!(
                    "{prefix}and {ty} {}, {}",
                    self.value_text(*lhs, value_order),
                    self.value_text(*rhs, value_order)
                )
            }
            Instruction::Alloca { ty } => format!("{prefix}alloca {ty}"),
            Instruction::Store { ty, value, ptr } => {
                format!(
                    "store {ty} {}, ptr {}",
                    self.value_text(*value, value_order),
                    self.value_text(*ptr, value_order)
                )
            }
            Instruction::Load { ty, ptr } => {
                format!(
                    "{prefix}load {ty}, ptr {}",
                    self.value_text(*ptr, value_order)
                )
            }
            Instruction::Icmp { pred, ty, lhs, rhs } => {
                format!(
                    "{prefix}icmp {pred} {ty} {}, {}",
                    self.value_text(*lhs, value_order),
                    self.value_text(*rhs, value_order)
                )
            }
            Instruction::Call {
                ret_ty,
                function,
                args,
            } => {
                let mut rendered = Vec::new();
                for (arg_ty, arg_value) in args {
                    rendered.push(format!(
                        "{arg_ty} {}",
                        self.value_text(*arg_value, value_order)
                    ));
                }

                if *ret_ty == Type::Void {
                    format!("call void @{function}({})", rendered.join(", "))
                } else {
                    format!("{prefix}call {ret_ty} @{function}({})", rendered.join(", "))
                }
            }
            Instruction::Phi { ty, incomings } => {
                let mut rendered = Vec::new();
                for incoming in incomings {
                    rendered.push(format!(
                        "[ {}, .{} ]",
                        self.value_text(incoming.value, value_order),
                        self.block_label(incoming.block)
                    ));
                }
                format!("{prefix}phi {ty} {}", rendered.join(", "))
            }
            Instruction::Jmp { target } => format!("jmp .{}", self.block_label(*target)),
            Instruction::Br {
                cond,
                then_block,
                else_block,
            } => {
                format!(
                    "br i8 {}, .{}, .{}",
                    self.value_text(*cond, value_order),
                    self.block_label(*then_block),
                    self.block_label(*else_block)
                )
            }
            Instruction::Ret { value } => {
                if let Some(value) = value {
                    let ty = self.value_type(*value).unwrap_or(Type::Void);
                    format!("ret {ty} {}", self.value_text(*value, value_order))
                } else {
                    "ret void".to_string()
                }
            }
        }
    }

    pub fn format_il(&self) -> String {
        let mut value_order = HashMap::new();
        let mut next_value = 0usize;
        for value in self.values.keys() {
            if matches!(
                self.values.get(value).map(|entry| &entry.kind),
                Some(ValueKind::InstructionResult(_))
            ) {
                value_order.insert(value, next_value);
                next_value += 1;
            }
        }

        let mut rendered_params = Vec::new();
        for param in &self.params {
            rendered_params.push(self.typed_value_text(param.value, &value_order));
        }

        let mut out = String::new();
        out.push_str(&format!(
            "func @{}({}) -> {} {{\n",
            self.name,
            rendered_params.join(", "),
            self.return_type
        ));

        for block in &self.block_order {
            let Some(block_data) = self.blocks.get(*block) else {
                continue;
            };

            out.push_str(&format!(".{}:\n", block_data.name));
            for instr_id in &block_data.instructions {
                if let Some(instruction) = self.instructions.get(*instr_id) {
                    out.push_str("    ");
                    out.push_str(&self.format_instruction(instruction, *instr_id, &value_order));
                    out.push('\n');
                }
            }
            out.push('\n');
        }

        out.push_str("}\n");
        out
    }
}

#[derive(Debug, Clone)]
pub struct IrBuilder {
    function: Function,
    current_block: Option<BlockId>,
}

impl IrBuilder {
    pub fn new(name: impl Into<String>, return_type: Type) -> Self {
        Self {
            function: Function::new(name, return_type),
            current_block: None,
        }
    }

    pub fn finish(self) -> Function {
        self.function
    }

    pub fn function(&self) -> &Function {
        &self.function
    }

    pub fn current_block(&self) -> Option<BlockId> {
        self.current_block
    }

    pub fn add_param(&mut self, name: impl Into<String>, ty: Type) -> ValueId {
        self.function.add_param(name, ty)
    }

    pub fn create_block(&mut self, name: impl Into<String>) -> BlockId {
        self.function.create_block(name)
    }

    pub fn position_at_end(&mut self, block: BlockId) -> Result<(), IrBuildError> {
        if self.function.blocks.contains_key(block) {
            self.current_block = Some(block);
            Ok(())
        } else {
            Err(IrBuildError::UnknownBlock(block))
        }
    }

    pub fn build_const(&mut self, ty: Type, value: i64) -> Result<ValueId, IrBuildError> {
        if !matches!(ty, Type::I8 | Type::I32 | Type::I64) {
            return Err(IrBuildError::InvalidType {
                context: "build_const",
                ty,
            });
        }

        Ok(self.function.values.insert(ValueData {
            ty,
            kind: ValueKind::ConstantInt(value),
        }))
    }

    pub fn build_const_i8(&mut self, value: i8) -> Result<ValueId, IrBuildError> {
        self.build_const(Type::I8, value as i64)
    }

    pub fn build_const_i32(&mut self, value: i32) -> Result<ValueId, IrBuildError> {
        self.build_const(Type::I32, value as i64)
    }

    pub fn build_const_i64(&mut self, value: i64) -> Result<ValueId, IrBuildError> {
        self.build_const(Type::I64, value)
    }

    pub fn build_add(&mut self, lhs: ValueId, rhs: ValueId) -> Result<ValueId, IrBuildError> {
        self.build_integer_binop("build_add", lhs, rhs, |ty, left, right| Instruction::Add {
            ty,
            lhs: left,
            rhs: right,
        })
    }

    pub fn build_sub(&mut self, lhs: ValueId, rhs: ValueId) -> Result<ValueId, IrBuildError> {
        self.build_integer_binop("build_sub", lhs, rhs, |ty, left, right| Instruction::Sub {
            ty,
            lhs: left,
            rhs: right,
        })
    }

    pub fn build_mul(&mut self, lhs: ValueId, rhs: ValueId) -> Result<ValueId, IrBuildError> {
        self.build_integer_binop("build_mul", lhs, rhs, |ty, left, right| Instruction::Mul {
            ty,
            lhs: left,
            rhs: right,
        })
    }

    pub fn build_sdiv(&mut self, lhs: ValueId, rhs: ValueId) -> Result<ValueId, IrBuildError> {
        self.build_integer_binop("build_sdiv", lhs, rhs, |ty, left, right| {
            Instruction::Sdiv {
                ty,
                lhs: left,
                rhs: right,
            }
        })
    }

    pub fn build_and(&mut self, lhs: ValueId, rhs: ValueId) -> Result<ValueId, IrBuildError> {
        self.build_integer_binop("build_and", lhs, rhs, |ty, left, right| Instruction::And {
            ty,
            lhs: left,
            rhs: right,
        })
    }

    pub fn build_alloca(&mut self, ty: Type) -> Result<ValueId, IrBuildError> {
        if ty == Type::Void {
            return Err(IrBuildError::InvalidType {
                context: "build_alloca",
                ty,
            });
        }

        self.append_with_value(Instruction::Alloca { ty })
    }

    pub fn build_store(
        &mut self,
        ty: Type,
        value: ValueId,
        ptr: ValueId,
    ) -> Result<(), IrBuildError> {
        self.ensure_value_type(value, ty, "build_store(value)")?;
        self.ensure_value_type(ptr, Type::Ptr, "build_store(ptr)")?;

        self.append(Instruction::Store { ty, value, ptr })?;
        Ok(())
    }

    pub fn build_load(&mut self, ty: Type, ptr: ValueId) -> Result<ValueId, IrBuildError> {
        self.ensure_value_type(ptr, Type::Ptr, "build_load(ptr)")?;
        self.append_with_value(Instruction::Load { ty, ptr })
    }

    pub fn build_icmp(
        &mut self,
        pred: IcmpPredicate,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, IrBuildError> {
        self.ensure_integer_type(ty, "build_icmp")?;
        self.ensure_value_type(lhs, ty, "build_icmp(lhs)")?;
        self.ensure_value_type(rhs, ty, "build_icmp(rhs)")?;

        self.append_with_value(Instruction::Icmp { pred, ty, lhs, rhs })
    }

    pub fn build_call(
        &mut self,
        ret_ty: Type,
        function: impl Into<String>,
        args: Vec<(Type, ValueId)>,
    ) -> Result<Option<ValueId>, IrBuildError> {
        for (arg_ty, arg_value) in &args {
            self.ensure_value_type(*arg_value, *arg_ty, "build_call(arg)")?;
        }

        self.append(Instruction::Call {
            ret_ty,
            function: function.into(),
            args,
        })
    }

    pub fn build_phi(
        &mut self,
        ty: Type,
        incomings: Vec<PhiIncoming>,
    ) -> Result<ValueId, IrBuildError> {
        for incoming in &incomings {
            self.ensure_value_type(incoming.value, ty, "build_phi(incoming)")?;
            if !self.function.blocks.contains_key(incoming.block) {
                return Err(IrBuildError::UnknownBlock(incoming.block));
            }
        }

        self.append_with_value(Instruction::Phi { ty, incomings })
    }

    pub fn add_phi_incoming(
        &mut self,
        phi_value: ValueId,
        incoming: PhiIncoming,
    ) -> Result<(), IrBuildError> {
        if !self.function.blocks.contains_key(incoming.block) {
            return Err(IrBuildError::UnknownBlock(incoming.block));
        }

        let phi_ty = self
            .function
            .value_type(phi_value)
            .ok_or(IrBuildError::UnknownValue(phi_value))?;
        self.ensure_value_type(incoming.value, phi_ty, "add_phi_incoming")?;

        let instr_id = match self
            .function
            .value(phi_value)
            .ok_or(IrBuildError::UnknownValue(phi_value))?
            .kind
        {
            ValueKind::InstructionResult(instr_id) => instr_id,
            _ => return Err(IrBuildError::ValueDoesNotComeFromInstruction(phi_value)),
        };

        let instruction = self
            .function
            .instructions
            .get_mut(instr_id)
            .ok_or(IrBuildError::ValueDoesNotComeFromInstruction(phi_value))?;

        match instruction {
            Instruction::Phi { incomings, .. } => {
                incomings.push(incoming);
                Ok(())
            }
            _ => Err(IrBuildError::NotAPhi(phi_value)),
        }
    }

    pub fn build_jmp(&mut self, target: BlockId) -> Result<(), IrBuildError> {
        if !self.function.blocks.contains_key(target) {
            return Err(IrBuildError::UnknownBlock(target));
        }

        self.append(Instruction::Jmp { target })?;
        Ok(())
    }

    pub fn build_br(
        &mut self,
        cond: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    ) -> Result<(), IrBuildError> {
        self.ensure_value_type(cond, Type::I8, "build_br(cond)")?;
        if !self.function.blocks.contains_key(then_block) {
            return Err(IrBuildError::UnknownBlock(then_block));
        }
        if !self.function.blocks.contains_key(else_block) {
            return Err(IrBuildError::UnknownBlock(else_block));
        }

        self.append(Instruction::Br {
            cond,
            then_block,
            else_block,
        })?;
        Ok(())
    }

    pub fn build_ret(&mut self, value: Option<ValueId>) -> Result<(), IrBuildError> {
        match value {
            Some(result) => {
                self.ensure_value_type(result, self.function.return_type, "build_ret(value)")?;
            }
            None => {
                if self.function.return_type != Type::Void {
                    return Err(IrBuildError::TypeMismatch {
                        context: "build_ret(void)",
                        expected: self.function.return_type,
                        found: Type::Void,
                    });
                }
            }
        }

        self.append(Instruction::Ret { value })?;
        Ok(())
    }

    fn append(&mut self, instruction: Instruction) -> Result<Option<ValueId>, IrBuildError> {
        let block = self
            .current_block
            .ok_or(IrBuildError::MissingCurrentBlock)?;
        self.function.append_instruction(block, instruction)
    }

    fn append_with_value(&mut self, instruction: Instruction) -> Result<ValueId, IrBuildError> {
        self.append(instruction)?.ok_or(IrBuildError::InvalidType {
            context: "append_with_value",
            ty: Type::Void,
        })
    }

    fn ensure_value_type(
        &self,
        value: ValueId,
        expected: Type,
        context: &'static str,
    ) -> Result<(), IrBuildError> {
        let found = self
            .function
            .value_type(value)
            .ok_or(IrBuildError::UnknownValue(value))?;
        if found == expected {
            Ok(())
        } else {
            Err(IrBuildError::TypeMismatch {
                context,
                expected,
                found,
            })
        }
    }

    fn ensure_integer_type(&self, ty: Type, context: &'static str) -> Result<(), IrBuildError> {
        if matches!(ty, Type::I8 | Type::I32 | Type::I64) {
            Ok(())
        } else {
            Err(IrBuildError::InvalidType { context, ty })
        }
    }

    fn build_integer_binop<F>(
        &mut self,
        context: &'static str,
        lhs: ValueId,
        rhs: ValueId,
        build: F,
    ) -> Result<ValueId, IrBuildError>
    where
        F: FnOnce(Type, ValueId, ValueId) -> Instruction,
    {
        let lhs_ty = self
            .function
            .value_type(lhs)
            .ok_or(IrBuildError::UnknownValue(lhs))?;
        let rhs_ty = self
            .function
            .value_type(rhs)
            .ok_or(IrBuildError::UnknownValue(rhs))?;

        if lhs_ty != rhs_ty {
            return Err(IrBuildError::TypeMismatch {
                context,
                expected: lhs_ty,
                found: rhs_ty,
            });
        }
        self.ensure_integer_type(lhs_ty, context)?;

        self.append_with_value(build(lhs_ty, lhs, rhs))
    }
}

pub fn build_factorial_il() -> Result<Function, IrBuildError> {
    let mut builder = IrBuilder::new("factorial", Type::I32);
    let n = builder.add_param("n", Type::I32);

    let entry = builder.create_block("entry");
    let loop_header = builder.create_block("loop_header");
    let loop_body = builder.create_block("loop_body");
    let end = builder.create_block("end");

    builder.position_at_end(entry)?;
    let one = builder.build_const_i32(1)?;
    let is_base = builder.build_icmp(IcmpPredicate::Sle, Type::I32, n, one)?;
    builder.build_br(is_base, end, loop_header)?;

    builder.position_at_end(loop_header)?;
    let current_n = builder.build_phi(
        Type::I32,
        vec![PhiIncoming {
            value: n,
            block: entry,
        }],
    )?;
    let acc = builder.build_phi(
        Type::I32,
        vec![PhiIncoming {
            value: one,
            block: entry,
        }],
    )?;
    let cond = builder.build_icmp(IcmpPredicate::Sgt, Type::I32, current_n, one)?;
    builder.build_br(cond, loop_body, end)?;

    builder.position_at_end(loop_body)?;
    let next_acc = builder.build_mul(acc, current_n)?;
    let next_n = builder.build_sub(current_n, one)?;
    builder.build_jmp(loop_header)?;

    builder.add_phi_incoming(
        current_n,
        PhiIncoming {
            value: next_n,
            block: loop_body,
        },
    )?;
    builder.add_phi_incoming(
        acc,
        PhiIncoming {
            value: next_acc,
            block: loop_body,
        },
    )?;

    builder.position_at_end(end)?;
    let result = builder.build_phi(
        Type::I32,
        vec![
            PhiIncoming {
                value: one,
                block: entry,
            },
            PhiIncoming {
                value: acc,
                block: loop_header,
            },
        ],
    )?;
    builder.build_ret(Some(result))?;

    Ok(builder.finish())
}

pub fn constant_fold(mut function: Function) -> Function {
    let mut folded_instrs = HashSet::new();
    let mut folded_values = Vec::new();

    for block_id in function.block_order.clone() {
        let Some(block) = function.blocks.get(block_id) else {
            continue;
        };

        for instr_id in block.instructions.clone() {
            let Some(instruction) = function.instructions.get(instr_id).cloned() else {
                continue;
            };

            let Some(constant) = fold_instruction_constant(&function, &instruction) else {
                continue;
            };

            let Some(result_value) = function.instr_results.get(&instr_id).copied() else {
                continue;
            };

            folded_instrs.insert(instr_id);
            folded_values.push((result_value, constant));
        }
    }

    for (value, constant) in folded_values {
        if let Some(data) = function.values.get_mut(value) {
            data.kind = ValueKind::ConstantInt(constant);
        }
    }

    for (_, block) in &mut function.blocks {
        block
            .instructions
            .retain(|instr| !folded_instrs.contains(instr));
    }

    rebuild_instr_results(&mut function);
    function
}

pub fn dead_code_elimination(mut function: Function) -> Function {
    let mut live_instrs = HashSet::new();
    let mut worklist = VecDeque::new();

    for (instr_id, instruction) in &function.instructions {
        if instruction.has_side_effects() {
            live_instrs.insert(instr_id);
            worklist.push_back(instr_id);
        }
    }

    while let Some(instr_id) = worklist.pop_front() {
        let Some(instruction) = function.instructions.get(instr_id).cloned() else {
            continue;
        };

        for value in instruction_used_values(&instruction) {
            let Some(value_data) = function.values.get(value) else {
                continue;
            };

            let ValueKind::InstructionResult(dep_instr) = value_data.kind else {
                continue;
            };

            if live_instrs.insert(dep_instr) {
                worklist.push_back(dep_instr);
            }
        }
    }

    for (_, block) in &mut function.blocks {
        block
            .instructions
            .retain(|instr| live_instrs.contains(instr));
    }

    let dead_instrs: Vec<_> = function
        .instructions
        .keys()
        .filter(|instr_id| !live_instrs.contains(instr_id))
        .collect();
    for instr_id in dead_instrs {
        function.instructions.remove(instr_id);
    }

    let dead_values: Vec<_> = function
        .values
        .iter()
        .filter_map(|(value_id, value_data)| match value_data.kind {
            ValueKind::InstructionResult(instr_id) if !live_instrs.contains(&instr_id) => {
                Some(value_id)
            }
            _ => None,
        })
        .collect();
    for value_id in dead_values {
        function.values.remove(value_id);
    }

    rebuild_instr_results(&mut function);

    function
}

pub fn simplify_cfg(mut function: Function) -> Function {
    loop {
        let predecessor_counts = predecessor_counts(&function);
        let mut changed = false;

        for block_id in function.block_order.clone() {
            let Some(block) = function.blocks.get(block_id) else {
                continue;
            };

            let Some(last_instr) = block.instructions.last().copied() else {
                continue;
            };

            let target = match function.instructions.get(last_instr) {
                Some(Instruction::Jmp { target }) => *target,
                _ => continue,
            };

            if target == block_id {
                continue;
            }
            if predecessor_counts.get(&target).copied().unwrap_or(0) != 1 {
                continue;
            }
            if block_starts_with_phi(&function, target) {
                continue;
            }

            let Some(target_block) = function.blocks.get(target) else {
                continue;
            };
            let target_instructions = target_block.instructions.clone();

            if let Some(block_mut) = function.blocks.get_mut(block_id) {
                if block_mut.instructions.last().copied() == Some(last_instr) {
                    block_mut.instructions.pop();
                }
                block_mut.instructions.extend(target_instructions);
            }

            function.blocks.remove(target);
            function.block_order.retain(|block| *block != target);

            for (_, instruction) in &mut function.instructions {
                if let Instruction::Phi { incomings, .. } = instruction {
                    for incoming in incomings {
                        if incoming.block == target {
                            incoming.block = block_id;
                        }
                    }
                }
            }

            changed = true;
            break;
        }

        if !changed {
            break;
        }

        rebuild_cfg_metadata(&mut function);
    }

    rebuild_cfg_metadata(&mut function);
    function
}

pub fn run_phase5_pipeline(function: Function) -> Function {
    let function = constant_fold(function);
    let function = dead_code_elimination(function);
    simplify_cfg(function)
}

fn fold_instruction_constant(function: &Function, instruction: &Instruction) -> Option<i64> {
    match instruction {
        Instruction::Add { lhs, rhs, .. } => {
            Some(constant_int(function, *lhs)? + constant_int(function, *rhs)?)
        }
        Instruction::Sub { lhs, rhs, .. } => {
            Some(constant_int(function, *lhs)? - constant_int(function, *rhs)?)
        }
        Instruction::Mul { lhs, rhs, .. } => {
            Some(constant_int(function, *lhs)? * constant_int(function, *rhs)?)
        }
        Instruction::Sdiv { lhs, rhs, .. } => {
            let rhs_value = constant_int(function, *rhs)?;
            if rhs_value == 0 {
                None
            } else {
                Some(constant_int(function, *lhs)? / rhs_value)
            }
        }
        Instruction::And { lhs, rhs, .. } => {
            Some(constant_int(function, *lhs)? & constant_int(function, *rhs)?)
        }
        Instruction::Icmp { pred, lhs, rhs, .. } => {
            let lhs = constant_int(function, *lhs)?;
            let rhs = constant_int(function, *rhs)?;
            let result = match pred {
                IcmpPredicate::Eq => lhs == rhs,
                IcmpPredicate::Ne => lhs != rhs,
                IcmpPredicate::Slt => lhs < rhs,
                IcmpPredicate::Sle => lhs <= rhs,
                IcmpPredicate::Sgt => lhs > rhs,
                IcmpPredicate::Sge => lhs >= rhs,
            };
            Some(if result { 1 } else { 0 })
        }
        _ => None,
    }
}

fn constant_int(function: &Function, value: ValueId) -> Option<i64> {
    let data = function.values.get(value)?;
    match data.kind {
        ValueKind::ConstantInt(value) => Some(value),
        _ => None,
    }
}

fn instruction_used_values(instruction: &Instruction) -> Vec<ValueId> {
    match instruction {
        Instruction::Add { lhs, rhs, .. }
        | Instruction::Sub { lhs, rhs, .. }
        | Instruction::Mul { lhs, rhs, .. }
        | Instruction::Sdiv { lhs, rhs, .. }
        | Instruction::And { lhs, rhs, .. }
        | Instruction::Icmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        Instruction::Store { value, ptr, .. } => vec![*value, *ptr],
        Instruction::Load { ptr, .. } => vec![*ptr],
        Instruction::Call { args, .. } => args.iter().map(|(_, value)| *value).collect(),
        Instruction::Phi { incomings, .. } => {
            incomings.iter().map(|incoming| incoming.value).collect()
        }
        Instruction::Br { cond, .. } => vec![*cond],
        Instruction::Ret { value } => value.iter().copied().collect(),
        Instruction::Alloca { .. } | Instruction::Jmp { .. } => Vec::new(),
    }
}

fn predecessor_counts(function: &Function) -> HashMap<BlockId, usize> {
    let mut counts = HashMap::new();
    for (block_id, _) in &function.blocks {
        counts.entry(block_id).or_insert(0);
    }

    for (_, block) in &function.blocks {
        let Some(last_instr) = block.instructions.last() else {
            continue;
        };

        let Some(instruction) = function.instructions.get(*last_instr) else {
            continue;
        };

        match instruction {
            Instruction::Jmp { target } => {
                *counts.entry(*target).or_insert(0) += 1;
            }
            Instruction::Br {
                then_block,
                else_block,
                ..
            } => {
                *counts.entry(*then_block).or_insert(0) += 1;
                *counts.entry(*else_block).or_insert(0) += 1;
            }
            _ => {}
        }
    }

    counts
}

fn block_starts_with_phi(function: &Function, block: BlockId) -> bool {
    let Some(block_data) = function.blocks.get(block) else {
        return false;
    };

    let Some(first_instr) = block_data.instructions.first() else {
        return false;
    };

    matches!(
        function.instructions.get(*first_instr),
        Some(Instruction::Phi { .. })
    )
}

fn rebuild_instr_results(function: &mut Function) {
    function.instr_results.clear();
    for (value_id, value_data) in &function.values {
        if let ValueKind::InstructionResult(instr_id) = value_data.kind {
            function.instr_results.insert(instr_id, value_id);
        }
    }
}

fn rebuild_cfg_metadata(function: &mut Function) {
    function.cfg = DiGraph::new();
    function.block_nodes.clear();

    function
        .block_order
        .retain(|block_id| function.blocks.contains_key(*block_id));

    for block_id in function.block_order.clone() {
        let node = function.cfg.add_node(block_id);
        function.block_nodes.insert(block_id, node);
    }

    for block_id in function.block_order.clone() {
        let Some(block) = function.blocks.get(block_id) else {
            continue;
        };
        let Some(last_instr) = block.instructions.last() else {
            continue;
        };
        let targets: Vec<BlockId> = match function.instructions.get(*last_instr) {
            Some(Instruction::Jmp { target }) => vec![*target],
            Some(Instruction::Br {
                then_block,
                else_block,
                ..
            }) => vec![*then_block, *else_block],
            _ => Vec::new(),
        };

        for target in targets {
            if function.block_nodes.contains_key(&target) {
                let _ = function.add_edge(block_id, target);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_side_effect_flags() {
        let add = Instruction::Add {
            ty: Type::I32,
            lhs: ValueId::default(),
            rhs: ValueId::default(),
        };
        let call = Instruction::Call {
            ret_ty: Type::Void,
            function: "foo".to_string(),
            args: Vec::new(),
        };

        assert!(!add.has_side_effects());
        assert!(call.has_side_effects());
    }

    #[test]
    fn builds_and_formats_factorial() {
        let function = build_factorial_il().expect("factorial construction should succeed");
        let text = function.format_il();
        println!("{text}");

        assert!(text.contains("func @factorial(i32 %n) -> i32"));
        assert!(text.contains(".loop_header:"));
        assert!(text.contains("phi i32"));
        assert!(text.contains("ret i32"));
        assert_eq!(function.cfg.edge_count(), 5);
    }

    #[test]
    fn constant_fold_folds_simple_add() {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder
            .position_at_end(entry)
            .expect("entry block should exist");
        let a = builder.build_const_i32(5).expect("const should build");
        let b = builder.build_const_i32(5).expect("const should build");
        let sum = builder.build_add(a, b).expect("add should build");
        builder.build_ret(Some(sum)).expect("ret should build");

        let folded = constant_fold(builder.finish());
        let text = folded.format_il();

        assert!(!text.contains("add i32"));
        assert!(text.contains("ret i32 10"));
    }

    #[test]
    fn dce_removes_unused_pure_instructions() {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder
            .position_at_end(entry)
            .expect("entry block should exist");
        let a = builder.build_const_i32(7).expect("const should build");
        let b = builder.build_const_i32(9).expect("const should build");
        let _unused = builder.build_mul(a, b).expect("mul should build");
        let ret_val = builder.build_const_i32(1).expect("const should build");
        builder
            .build_ret(Some(ret_val))
            .expect("return should build");

        let dce = dead_code_elimination(builder.finish());
        let text = dce.format_il();

        assert!(!text.contains("mul i32"));
        assert!(text.contains("ret i32 1"));
    }

    #[test]
    fn cfg_simplify_merges_linear_jump_blocks() {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        let mid = builder.create_block("mid");
        let end = builder.create_block("end");

        builder
            .position_at_end(entry)
            .expect("entry block should exist");
        builder.build_jmp(mid).expect("jmp should build");

        builder
            .position_at_end(mid)
            .expect("mid block should exist");
        builder.build_jmp(end).expect("jmp should build");

        builder
            .position_at_end(end)
            .expect("end block should exist");
        let ret_val = builder.build_const_i32(2).expect("const should build");
        builder.build_ret(Some(ret_val)).expect("ret should build");

        let simplified = simplify_cfg(builder.finish());

        assert_eq!(simplified.blocks.len(), 1);
        assert_eq!(simplified.cfg.edge_count(), 0);
        let text = simplified.format_il();
        assert!(text.contains("ret i32 2"));
    }

    #[test]
    fn phase5_pipeline_preserves_loop_carried_defs() {
        let factorial = build_factorial_il().expect("factorial should build");
        let optimized = run_phase5_pipeline(factorial);
        let text = optimized.format_il();

        assert!(text.contains("mul i32"));
        assert!(text.contains("sub i32"));
    }
}
