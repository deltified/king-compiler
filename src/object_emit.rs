use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use object::write::{Object, Relocation, StandardSection, Symbol, SymbolId, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, RelocationEncoding, RelocationFlags, RelocationKind,
    SymbolFlags, SymbolKind, SymbolScope,
};

use crate::mir::{Cond, MirFunction, MirInst, Operand, PhysReg, Reg, TargetArch};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectEmitError {
    message: String,
}

impl ObjectEmitError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ObjectEmitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for ObjectEmitError {}

#[derive(Debug, Clone)]
struct RelocRequest {
    offset: u64,
    symbol: String,
    kind: RelocationKind,
    encoding: RelocationEncoding,
    size: u8,
    addend: i64,
}

pub fn emit_object_file(
    func: &MirFunction,
    target: TargetArch,
) -> Result<Vec<u8>, ObjectEmitError> {
    match target {
        TargetArch::Arm64 => {
            let (bytes, relocs) = encode_arm64_function(func)?;
            build_object(func, Architecture::Aarch64, &bytes, 4, &relocs)
        }
        TargetArch::Amd64 | TargetArch::X86_64 => {
            let (bytes, relocs) = encode_x86_64_function(func)?;
            build_object(func, Architecture::X86_64, &bytes, 1, &relocs)
        }
    }
}

fn build_object(
    func: &MirFunction,
    architecture: Architecture,
    text_bytes: &[u8],
    text_align: u64,
    relocs: &[RelocRequest],
) -> Result<Vec<u8>, ObjectEmitError> {
    let mut object = Object::new(host_binary_format(), architecture, Endianness::Little);
    object.add_file_symbol(b"king_compiler_phase6".to_vec());

    let text_section = object.section_id(StandardSection::Text);
    let text_offset = object.append_section_data(text_section, text_bytes, text_align);

    let mut symbols = HashMap::new();
    let func_symbol = object.add_symbol(Symbol {
        name: func.name.clone().into_bytes(),
        value: text_offset,
        size: text_bytes.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Section(text_section),
        flags: SymbolFlags::None,
    });
    symbols.insert(func.name.clone(), func_symbol);

    for reloc in relocs {
        let symbol_id = get_or_create_symbol(&mut object, &mut symbols, &reloc.symbol);
        object
            .add_relocation(
                text_section,
                Relocation {
                    offset: text_offset + reloc.offset,
                    symbol: symbol_id,
                    addend: reloc.addend,
                    flags: RelocationFlags::Generic {
                        kind: reloc.kind,
                        encoding: reloc.encoding,
                        size: reloc.size,
                    },
                },
            )
            .map_err(|err| {
                ObjectEmitError::new(format!(
                    "failed to add relocation at text+{} for symbol {}: {err}",
                    reloc.offset, reloc.symbol
                ))
            })?;
    }

    object
        .write()
        .map_err(|err| ObjectEmitError::new(format!("failed to write object bytes: {err}")))
}

fn get_or_create_symbol(
    object: &mut Object<'_>,
    symbols: &mut HashMap<String, SymbolId>,
    name: &str,
) -> SymbolId {
    if let Some(id) = symbols.get(name).copied() {
        return id;
    }

    let id = object.add_symbol(Symbol {
        name: name.as_bytes().to_vec(),
        value: 0,
        size: 0,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    symbols.insert(name.to_string(), id);
    id
}

fn host_binary_format() -> BinaryFormat {
    if cfg!(target_os = "macos") {
        BinaryFormat::MachO
    } else if cfg!(target_os = "windows") {
        BinaryFormat::Coff
    } else {
        BinaryFormat::Elf
    }
}

#[derive(Debug, Clone)]
struct Arm64BranchFixup {
    word_index: usize,
    label: String,
    kind: Arm64BranchFixupKind,
}

#[derive(Debug, Clone, Copy)]
enum Arm64BranchFixupKind {
    B,
    BCond(Cond),
}

fn encode_arm64_function(
    func: &MirFunction,
) -> Result<(Vec<u8>, Vec<RelocRequest>), ObjectEmitError> {
    let mut words = Vec::new();
    let mut labels = HashMap::new();
    let mut fixups = Vec::new();
    let mut call_relocs = Vec::new();

    for inst in &func.instructions {
        match inst {
            MirInst::Label(label) => {
                labels.insert(label.clone(), words.len());
            }
            MirInst::Mov { dst, src } => {
                emit_arm64_mov(*dst, src, &mut words)?;
            }
            MirInst::Add { dst, lhs, rhs } => {
                emit_arm64_add_sub(*dst, *lhs, rhs, false, &mut words)?;
            }
            MirInst::Sub { dst, lhs, rhs } => {
                emit_arm64_add_sub(*dst, *lhs, rhs, true, &mut words)?;
            }
            MirInst::And { dst, lhs, rhs } => {
                let rd = arm64_reg_code(*dst)?;
                let rn = arm64_reg_code(*lhs)?;
                let rm = arm64_reg_from_operand(rhs)?;
                ensure_arm64_gpr_not_sp("and lhs", rn)?;
                ensure_arm64_gpr_not_sp("and rhs", rm)?;
                ensure_arm64_gpr_not_sp("and dst", rd)?;
                words.push(encode_arm64_and_reg(rd, rn, rm));
            }
            MirInst::Mul { dst, lhs, rhs } => {
                let rd = arm64_reg_code(*dst)?;
                let rn = arm64_reg_code(*lhs)?;
                let rm = arm64_reg_from_operand(rhs)?;
                ensure_arm64_gpr_not_sp("mul lhs", rn)?;
                ensure_arm64_gpr_not_sp("mul rhs", rm)?;
                ensure_arm64_gpr_not_sp("mul dst", rd)?;
                words.push(encode_arm64_mul(rd, rn, rm));
            }
            MirInst::Sdiv { dst, lhs, rhs } => {
                let rd = arm64_reg_code(*dst)?;
                let rn = arm64_reg_code(*lhs)?;
                let rm = arm64_reg_from_operand(rhs)?;
                ensure_arm64_gpr_not_sp("sdiv lhs", rn)?;
                ensure_arm64_gpr_not_sp("sdiv rhs", rm)?;
                ensure_arm64_gpr_not_sp("sdiv dst", rd)?;
                words.push(encode_arm64_sdiv(rd, rn, rm));
            }
            MirInst::Cmp { lhs, rhs } => {
                let rn = arm64_reg_code(*lhs)?;
                ensure_arm64_gpr_not_sp("cmp lhs", rn)?;
                match rhs {
                    Operand::Imm(imm) => words.push(encode_arm64_cmp_imm(rn, *imm)?),
                    Operand::Reg(reg) => {
                        let rm = arm64_reg_code(*reg)?;
                        ensure_arm64_gpr_not_sp("cmp rhs", rm)?;
                        words.push(encode_arm64_cmp_reg(rn, rm));
                    }
                }
            }
            MirInst::LoadStack { dst, offset } => {
                let rt = arm64_reg_code(*dst)?;
                let signed_offset = offset
                    .checked_neg()
                    .ok_or_else(|| ObjectEmitError::new("load stack offset overflow"))?;
                words.push(encode_arm64_ldur(rt, 29, signed_offset)?);
            }
            MirInst::StoreStack { src, offset } => {
                let rt = arm64_reg_code(*src)?;
                let signed_offset = offset
                    .checked_neg()
                    .ok_or_else(|| ObjectEmitError::new("store stack offset overflow"))?;
                words.push(encode_arm64_stur(rt, 29, signed_offset)?);
            }
            MirInst::Jmp { label } => {
                fixups.push(Arm64BranchFixup {
                    word_index: words.len(),
                    label: label.clone(),
                    kind: Arm64BranchFixupKind::B,
                });
                words.push(0);
            }
            MirInst::JmpIf { cond, label } => {
                fixups.push(Arm64BranchFixup {
                    word_index: words.len(),
                    label: label.clone(),
                    kind: Arm64BranchFixupKind::BCond(*cond),
                });
                words.push(0);
            }
            MirInst::Call { symbol } => {
                call_relocs.push(RelocRequest {
                    offset: (words.len() * 4) as u64,
                    symbol: symbol.clone(),
                    kind: RelocationKind::Relative,
                    encoding: RelocationEncoding::AArch64Call,
                    size: 26,
                    addend: 0,
                });
                words.push(0x9400_0000);
            }
            MirInst::Ret => words.push(encode_arm64_ret()),
            MirInst::Push { .. } | MirInst::Pop { .. } => {
                return Err(ObjectEmitError::new(
                    "push/pop pseudo-instructions are not supported in arm64 object emission",
                ));
            }
        }
    }

    for fixup in fixups {
        let target_index = labels.get(&fixup.label).copied().ok_or_else(|| {
            ObjectEmitError::new(format!(
                "unknown label referenced by arm64 branch: {}",
                fixup.label
            ))
        })?;
        match fixup.kind {
            Arm64BranchFixupKind::B => {
                let imm = arm64_branch_delta_words(fixup.word_index, target_index, 26)?;
                words[fixup.word_index] = 0x1400_0000 | ((imm as u32) & 0x03ff_ffff);
            }
            Arm64BranchFixupKind::BCond(cond) => {
                let imm = arm64_branch_delta_words(fixup.word_index, target_index, 19)?;
                words[fixup.word_index] = 0x5400_0000
                    | (((imm as u32) & 0x0007_ffff) << 5)
                    | (arm64_cond_bits(cond) as u32);
            }
        }
    }

    let mut bytes = Vec::with_capacity(words.len() * 4);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }

    Ok((bytes, call_relocs))
}

fn arm64_branch_delta_words(from: usize, to: usize, bits: u8) -> Result<i32, ObjectEmitError> {
    let delta = to as i64 - from as i64;
    let max = (1_i64 << (bits - 1)) - 1;
    let min = -(1_i64 << (bits - 1));
    if !(min..=max).contains(&delta) {
        return Err(ObjectEmitError::new(format!(
            "arm64 branch target out of range: delta={delta}, bits={bits}"
        )));
    }
    Ok(delta as i32)
}

fn arm64_cond_bits(cond: Cond) -> u8 {
    match cond {
        Cond::Eq => 0x0,
        Cond::Ne => 0x1,
        Cond::Ge => 0xA,
        Cond::Lt => 0xB,
        Cond::Gt => 0xC,
        Cond::Le => 0xD,
    }
}

fn arm64_reg_code(reg: Reg) -> Result<u8, ObjectEmitError> {
    let Reg::Phys(phys) = reg else {
        return Err(ObjectEmitError::new(
            "arm64 object emission requires physical registers",
        ));
    };
    Ok(match phys {
        PhysReg::X0 => 0,
        PhysReg::X1 => 1,
        PhysReg::X2 => 2,
        PhysReg::X3 => 3,
        PhysReg::X4 => 4,
        PhysReg::X5 => 5,
        PhysReg::X6 => 6,
        PhysReg::X7 => 7,
        PhysReg::X8 => 8,
        PhysReg::X9 => 9,
        PhysReg::X10 => 10,
        PhysReg::X11 => 11,
        PhysReg::X12 => 12,
        PhysReg::X13 => 13,
        PhysReg::X14 => 14,
        PhysReg::X15 => 15,
        PhysReg::X16 => 16,
        PhysReg::X17 => 17,
        PhysReg::X18 => 18,
        PhysReg::X19 => 19,
        PhysReg::X20 => 20,
        PhysReg::X21 => 21,
        PhysReg::X22 => 22,
        PhysReg::X23 => 23,
        PhysReg::X24 => 24,
        PhysReg::X25 => 25,
        PhysReg::X26 => 26,
        PhysReg::X27 => 27,
        PhysReg::X28 => 28,
        PhysReg::X29 => 29,
        PhysReg::X30 => 30,
        PhysReg::SP => 31,
        _ => {
            return Err(ObjectEmitError::new(
                "arm64 object emission received non-arm64 physical register",
            ));
        }
    })
}

fn ensure_arm64_gpr_not_sp(context: &'static str, reg: u8) -> Result<(), ObjectEmitError> {
    if reg == 31 {
        Err(ObjectEmitError::new(format!(
            "{context} cannot use SP in this encoding"
        )))
    } else {
        Ok(())
    }
}

fn arm64_reg_from_operand(operand: &Operand) -> Result<u8, ObjectEmitError> {
    match operand {
        Operand::Reg(reg) => arm64_reg_code(*reg),
        Operand::Imm(_) => Err(ObjectEmitError::new("expected register operand")),
    }
}

fn emit_arm64_mov(dst: Reg, src: &Operand, out: &mut Vec<u32>) -> Result<(), ObjectEmitError> {
    let rd = arm64_reg_code(dst)?;
    match src {
        Operand::Imm(imm) => emit_arm64_mov_imm(rd, *imm, out),
        Operand::Reg(src_reg) => {
            let rm = arm64_reg_code(*src_reg)?;
            if rd == 31 || rm == 31 {
                out.push(encode_arm64_add_sub_imm(false, rd, rm, 0)?);
            } else {
                out.push(encode_arm64_mov_reg(rd, rm));
            }
        }
    }
    Ok(())
}

fn emit_arm64_mov_imm(rd: u8, value: i64, out: &mut Vec<u32>) {
    let value = value as u64;
    let mut emitted = false;

    for hw in 0..4 {
        let part = ((value >> (hw * 16)) & 0xffff) as u16;
        if !emitted {
            if part == 0 && hw < 3 {
                continue;
            }
            out.push(encode_arm64_movz(rd, part, hw));
            emitted = true;
        } else if part != 0 {
            out.push(encode_arm64_movk(rd, part, hw));
        }
    }

    if !emitted {
        out.push(encode_arm64_movz(rd, 0, 0));
    }
}

fn emit_arm64_add_sub(
    dst: Reg,
    lhs: Reg,
    rhs: &Operand,
    is_sub: bool,
    out: &mut Vec<u32>,
) -> Result<(), ObjectEmitError> {
    let rd = arm64_reg_code(dst)?;
    let rn = arm64_reg_code(lhs)?;

    match rhs {
        Operand::Imm(imm) => {
            let mut value = *imm;
            let mut op_is_sub = is_sub;
            if value < 0 {
                value = -value;
                op_is_sub = !op_is_sub;
            }
            out.push(encode_arm64_add_sub_imm(op_is_sub, rd, rn, value as u64)?);
        }
        Operand::Reg(reg) => {
            let rm = arm64_reg_code(*reg)?;
            ensure_arm64_gpr_not_sp("add/sub rhs", rm)?;
            out.push(if is_sub {
                encode_arm64_sub_reg(rd, rn, rm)
            } else {
                encode_arm64_add_reg(rd, rn, rm)
            });
        }
    }

    Ok(())
}

fn encode_arm64_mov_reg(rd: u8, rm: u8) -> u32 {
    // mov rd, rm  == orr rd, xzr, rm
    0xAA00_03E0 | ((rm as u32) << 16) | (rd as u32)
}

fn encode_arm64_movz(rd: u8, imm16: u16, hw: u32) -> u32 {
    0xD280_0000 | ((hw & 0x3) << 21) | ((imm16 as u32) << 5) | (rd as u32)
}

fn encode_arm64_movk(rd: u8, imm16: u16, hw: u32) -> u32 {
    0xF280_0000 | ((hw & 0x3) << 21) | ((imm16 as u32) << 5) | (rd as u32)
}

fn encode_arm64_add_reg(rd: u8, rn: u8, rm: u8) -> u32 {
    0x8B00_0000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}

fn encode_arm64_sub_reg(rd: u8, rn: u8, rm: u8) -> u32 {
    0xCB00_0000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}

fn encode_arm64_and_reg(rd: u8, rn: u8, rm: u8) -> u32 {
    0x8A00_0000 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}

fn encode_arm64_mul(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9B00_7C00 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}

fn encode_arm64_sdiv(rd: u8, rn: u8, rm: u8) -> u32 {
    0x9AC0_0C00 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32)
}

fn encode_arm64_cmp_reg(rn: u8, rm: u8) -> u32 {
    // cmp rn, rm == subs xzr, rn, rm
    0xEB00_001F | ((rm as u32) << 16) | ((rn as u32) << 5)
}

fn encode_arm64_cmp_imm(rn: u8, imm: i64) -> Result<u32, ObjectEmitError> {
    if imm < 0 {
        return Err(ObjectEmitError::new("cmp immediate must be non-negative"));
    }
    let (imm12, shift) = encode_arm64_addsub_imm_fields(imm as u64)?;
    Ok(0xF100_001F | (shift << 22) | (imm12 << 10) | ((rn as u32) << 5))
}

fn encode_arm64_add_sub_imm(
    is_sub: bool,
    rd: u8,
    rn: u8,
    imm: u64,
) -> Result<u32, ObjectEmitError> {
    let (imm12, shift) = encode_arm64_addsub_imm_fields(imm)?;
    let base = if is_sub { 0xD100_0000 } else { 0x9100_0000 };
    Ok(base | (shift << 22) | (imm12 << 10) | ((rn as u32) << 5) | (rd as u32))
}

fn encode_arm64_addsub_imm_fields(imm: u64) -> Result<(u32, u32), ObjectEmitError> {
    if imm <= 4095 {
        Ok((imm as u32, 0))
    } else if imm % 4096 == 0 {
        let shifted = imm / 4096;
        if shifted <= 4095 {
            Ok((shifted as u32, 1))
        } else {
            Err(ObjectEmitError::new(format!(
                "immediate out of range for arm64 add/sub encoding: {imm}"
            )))
        }
    } else {
        Err(ObjectEmitError::new(format!(
            "immediate is not encodable for arm64 add/sub: {imm}"
        )))
    }
}

fn encode_arm64_ldur(rt: u8, rn: u8, imm9: i32) -> Result<u32, ObjectEmitError> {
    if !(-256..=255).contains(&imm9) {
        return Err(ObjectEmitError::new(format!(
            "stack load offset out of range for arm64 ldur: {imm9}"
        )));
    }
    let imm = (imm9 as i16 as u16 as u32) & 0x1ff;
    Ok(0xF840_0000 | (imm << 12) | ((rn as u32) << 5) | (rt as u32))
}

fn encode_arm64_stur(rt: u8, rn: u8, imm9: i32) -> Result<u32, ObjectEmitError> {
    if !(-256..=255).contains(&imm9) {
        return Err(ObjectEmitError::new(format!(
            "stack store offset out of range for arm64 stur: {imm9}"
        )));
    }
    let imm = (imm9 as i16 as u16 as u32) & 0x1ff;
    Ok(0xF800_0000 | (imm << 12) | ((rn as u32) << 5) | (rt as u32))
}

fn encode_arm64_ret() -> u32 {
    0xD65F_03C0
}

#[derive(Debug, Clone)]
struct X86BranchFixup {
    disp_offset: usize,
    next_instr_offset: usize,
    label: String,
}

fn encode_x86_64_function(
    func: &MirFunction,
) -> Result<(Vec<u8>, Vec<RelocRequest>), ObjectEmitError> {
    let mut bytes = Vec::new();
    let mut labels = HashMap::new();
    let mut branch_fixups = Vec::new();
    let mut call_relocs = Vec::new();

    for inst in &func.instructions {
        match inst {
            MirInst::Label(label) => {
                labels.insert(label.clone(), bytes.len());
            }
            MirInst::Mov { dst, src } => emit_x86_64_mov(*dst, src, &mut bytes)?,
            MirInst::Add { dst, lhs, rhs } => {
                emit_x86_64_add_sub_and(*dst, *lhs, rhs, X86AluOp::Add, &mut bytes)?
            }
            MirInst::Sub { dst, lhs, rhs } => {
                emit_x86_64_add_sub_and(*dst, *lhs, rhs, X86AluOp::Sub, &mut bytes)?
            }
            MirInst::And { dst, lhs, rhs } => {
                emit_x86_64_add_sub_and(*dst, *lhs, rhs, X86AluOp::And, &mut bytes)?
            }
            MirInst::Mul { dst, lhs, rhs } => emit_x86_64_mul(*dst, *lhs, rhs, &mut bytes)?,
            MirInst::Sdiv { dst, lhs, rhs } => emit_x86_64_sdiv(*dst, *lhs, rhs, &mut bytes)?,
            MirInst::Cmp { lhs, rhs } => emit_x86_64_cmp(*lhs, rhs, &mut bytes)?,
            MirInst::Push { src } => emit_x86_64_push(*src, &mut bytes)?,
            MirInst::Pop { dst } => emit_x86_64_pop(*dst, &mut bytes)?,
            MirInst::LoadStack { dst, offset } => {
                emit_x86_64_load_stack(*dst, *offset, &mut bytes)?
            }
            MirInst::StoreStack { src, offset } => {
                emit_x86_64_store_stack(*src, *offset, &mut bytes)?
            }
            MirInst::Jmp { label } => {
                let instr_offset = bytes.len();
                bytes.push(0xE9);
                bytes.extend_from_slice(&[0, 0, 0, 0]);
                branch_fixups.push(X86BranchFixup {
                    disp_offset: instr_offset + 1,
                    next_instr_offset: instr_offset + 5,
                    label: label.clone(),
                });
            }
            MirInst::JmpIf { cond, label } => {
                let instr_offset = bytes.len();
                bytes.push(0x0F);
                bytes.push(x86_64_jcc_opcode(*cond));
                bytes.extend_from_slice(&[0, 0, 0, 0]);
                branch_fixups.push(X86BranchFixup {
                    disp_offset: instr_offset + 2,
                    next_instr_offset: instr_offset + 6,
                    label: label.clone(),
                });
            }
            MirInst::Call { symbol } => {
                let instr_offset = bytes.len();
                bytes.push(0xE8);
                bytes.extend_from_slice(&[0, 0, 0, 0]);
                call_relocs.push(RelocRequest {
                    offset: (instr_offset + 1) as u64,
                    symbol: symbol.clone(),
                    kind: RelocationKind::Relative,
                    encoding: RelocationEncoding::X86Branch,
                    size: 32,
                    addend: -4,
                });
            }
            MirInst::Ret => bytes.push(0xC3),
        }
    }

    for fixup in branch_fixups {
        let target = labels.get(&fixup.label).copied().ok_or_else(|| {
            ObjectEmitError::new(format!(
                "unknown label referenced by x86_64 branch: {}",
                fixup.label
            ))
        })?;
        let disp = target as i64 - fixup.next_instr_offset as i64;
        let disp = i32::try_from(disp)
            .map_err(|_| ObjectEmitError::new(format!("x86_64 branch out of range: {disp}")))?;
        bytes[fixup.disp_offset..fixup.disp_offset + 4].copy_from_slice(&disp.to_le_bytes());
    }

    Ok((bytes, call_relocs))
}

#[derive(Debug, Clone, Copy)]
enum X86AluOp {
    Add,
    Sub,
    And,
}

fn x86_64_reg_code(reg: Reg) -> Result<u8, ObjectEmitError> {
    let Reg::Phys(phys) = reg else {
        return Err(ObjectEmitError::new(
            "x86_64 object emission requires physical registers",
        ));
    };

    match phys {
        PhysReg::RAX => Ok(0),
        PhysReg::RCX => Ok(1),
        PhysReg::RDX => Ok(2),
        PhysReg::RBX => Ok(3),
        PhysReg::RSP => Ok(4),
        PhysReg::RBP => Ok(5),
        PhysReg::RSI => Ok(6),
        PhysReg::RDI => Ok(7),
        PhysReg::R8 => Ok(8),
        PhysReg::R9 => Ok(9),
        PhysReg::R10 => Ok(10),
        PhysReg::R11 => Ok(11),
        PhysReg::R12 => Ok(12),
        PhysReg::R13 => Ok(13),
        PhysReg::R14 => Ok(14),
        PhysReg::R15 => Ok(15),
        _ => Err(ObjectEmitError::new(
            "x86_64 object emission received non-x86_64 physical register",
        )),
    }
}

fn emit_x86_rex(bytes: &mut Vec<u8>, w: bool, r: u8, x: u8, b: u8) {
    let mut rex = 0x40u8;
    if w {
        rex |= 0x08;
    }
    if (r & 0b1000) != 0 {
        rex |= 0x04;
    }
    if (x & 0b1000) != 0 {
        rex |= 0x02;
    }
    if (b & 0b1000) != 0 {
        rex |= 0x01;
    }
    if rex != 0x40 {
        bytes.push(rex);
    }
}

fn x86_modrm(mode: u8, reg: u8, rm: u8) -> u8 {
    (mode << 6) | ((reg & 7) << 3) | (rm & 7)
}

fn emit_x86_64_mov(dst: Reg, src: &Operand, bytes: &mut Vec<u8>) -> Result<(), ObjectEmitError> {
    let dst = x86_64_reg_code(dst)?;
    match src {
        Operand::Imm(imm) => {
            emit_x86_rex(bytes, true, 0, 0, dst);
            bytes.push(0xB8 + (dst & 7));
            bytes.extend_from_slice(&imm.to_le_bytes());
        }
        Operand::Reg(src) => {
            let src = x86_64_reg_code(*src)?;
            emit_x86_rex(bytes, true, src, 0, dst);
            bytes.push(0x89);
            bytes.push(x86_modrm(0b11, src, dst));
        }
    }
    Ok(())
}

fn emit_x86_64_add_sub_and(
    dst: Reg,
    lhs: Reg,
    rhs: &Operand,
    op: X86AluOp,
    bytes: &mut Vec<u8>,
) -> Result<(), ObjectEmitError> {
    let dst_code = x86_64_reg_code(dst)?;
    let lhs_code = x86_64_reg_code(lhs)?;

    if dst_code != lhs_code {
        emit_x86_64_mov(dst, &Operand::Reg(lhs), bytes)?;
    }

    match rhs {
        Operand::Reg(rhs_reg) => {
            let rhs_code = x86_64_reg_code(*rhs_reg)?;
            emit_x86_rex(bytes, true, rhs_code, 0, dst_code);
            bytes.push(match op {
                X86AluOp::Add => 0x01,
                X86AluOp::Sub => 0x29,
                X86AluOp::And => 0x21,
            });
            bytes.push(x86_modrm(0b11, rhs_code, dst_code));
        }
        Operand::Imm(imm) => {
            let imm32 = i32::try_from(*imm).map_err(|_| {
                ObjectEmitError::new(format!("x86_64 immediate out of 32-bit range: {imm}"))
            })?;
            emit_x86_rex(bytes, true, 0, 0, dst_code);
            bytes.push(0x81);
            let subop = match op {
                X86AluOp::Add => 0,
                X86AluOp::Sub => 5,
                X86AluOp::And => 4,
            };
            bytes.push(x86_modrm(0b11, subop, dst_code));
            bytes.extend_from_slice(&imm32.to_le_bytes());
        }
    }

    Ok(())
}

fn emit_x86_64_mul(
    dst: Reg,
    lhs: Reg,
    rhs: &Operand,
    bytes: &mut Vec<u8>,
) -> Result<(), ObjectEmitError> {
    let dst_code = x86_64_reg_code(dst)?;
    let lhs_code = x86_64_reg_code(lhs)?;

    if dst_code != lhs_code {
        emit_x86_64_mov(dst, &Operand::Reg(lhs), bytes)?;
    }

    match rhs {
        Operand::Reg(rhs_reg) => {
            let rhs_code = x86_64_reg_code(*rhs_reg)?;
            emit_x86_rex(bytes, true, dst_code, 0, rhs_code);
            bytes.push(0x0F);
            bytes.push(0xAF);
            bytes.push(x86_modrm(0b11, dst_code, rhs_code));
        }
        Operand::Imm(imm) => {
            let imm32 = i32::try_from(*imm).map_err(|_| {
                ObjectEmitError::new(format!("x86_64 immediate out of 32-bit range: {imm}"))
            })?;
            emit_x86_rex(bytes, true, dst_code, 0, dst_code);
            bytes.push(0x69);
            bytes.push(x86_modrm(0b11, dst_code, dst_code));
            bytes.extend_from_slice(&imm32.to_le_bytes());
        }
    }

    Ok(())
}

fn emit_x86_64_sdiv(
    dst: Reg,
    lhs: Reg,
    rhs: &Operand,
    bytes: &mut Vec<u8>,
) -> Result<(), ObjectEmitError> {
    let dst_code = x86_64_reg_code(dst)?;
    let lhs_code = x86_64_reg_code(lhs)?;
    const RAX: u8 = 0;
    const R11: u8 = 11;

    let mut rhs_code_override = None;
    if let Operand::Reg(rhs_reg) = rhs {
        let rhs_code = x86_64_reg_code(*rhs_reg)?;
        if rhs_code == RAX && lhs_code != RAX {
            emit_x86_64_mov(Reg::Phys(PhysReg::R11), &Operand::Reg(*rhs_reg), bytes)?;
            rhs_code_override = Some(R11);
        }
    }

    if lhs_code != RAX {
        emit_x86_64_mov(Reg::Phys(PhysReg::RAX), &Operand::Reg(lhs), bytes)?;
    }

    // cqto
    bytes.push(0x48);
    bytes.push(0x99);

    match rhs {
        Operand::Reg(rhs_reg) => {
            let rhs_code = rhs_code_override.unwrap_or(x86_64_reg_code(*rhs_reg)?);
            emit_x86_rex(bytes, true, 7, 0, rhs_code);
            bytes.push(0xF7);
            bytes.push(x86_modrm(0b11, 7, rhs_code));
        }
        Operand::Imm(imm) => {
            emit_x86_64_mov(Reg::Phys(PhysReg::R11), &Operand::Imm(*imm), bytes)?;
            emit_x86_rex(bytes, true, 7, 0, R11);
            bytes.push(0xF7);
            bytes.push(x86_modrm(0b11, 7, R11));
        }
    }

    if dst_code != RAX {
        emit_x86_64_mov(dst, &Operand::Reg(Reg::Phys(PhysReg::RAX)), bytes)?;
    }

    Ok(())
}

fn emit_x86_64_cmp(lhs: Reg, rhs: &Operand, bytes: &mut Vec<u8>) -> Result<(), ObjectEmitError> {
    let lhs_code = x86_64_reg_code(lhs)?;
    match rhs {
        Operand::Reg(rhs_reg) => {
            let rhs_code = x86_64_reg_code(*rhs_reg)?;
            emit_x86_rex(bytes, true, rhs_code, 0, lhs_code);
            bytes.push(0x39);
            bytes.push(x86_modrm(0b11, rhs_code, lhs_code));
        }
        Operand::Imm(imm) => {
            let imm32 = i32::try_from(*imm).map_err(|_| {
                ObjectEmitError::new(format!("x86_64 immediate out of 32-bit range: {imm}"))
            })?;
            emit_x86_rex(bytes, true, 7, 0, lhs_code);
            bytes.push(0x81);
            bytes.push(x86_modrm(0b11, 7, lhs_code));
            bytes.extend_from_slice(&imm32.to_le_bytes());
        }
    }

    Ok(())
}

fn emit_x86_64_push(src: Reg, bytes: &mut Vec<u8>) -> Result<(), ObjectEmitError> {
    let src = x86_64_reg_code(src)?;
    emit_x86_rex(bytes, false, 0, 0, src);
    bytes.push(0x50 + (src & 7));
    Ok(())
}

fn emit_x86_64_pop(dst: Reg, bytes: &mut Vec<u8>) -> Result<(), ObjectEmitError> {
    let dst = x86_64_reg_code(dst)?;
    emit_x86_rex(bytes, false, 0, 0, dst);
    bytes.push(0x58 + (dst & 7));
    Ok(())
}

fn emit_x86_64_load_stack(
    dst: Reg,
    offset: i32,
    bytes: &mut Vec<u8>,
) -> Result<(), ObjectEmitError> {
    let dst = x86_64_reg_code(dst)?;
    let disp = 0_i32
        .checked_sub(offset)
        .ok_or_else(|| ObjectEmitError::new("x86_64 stack load offset overflow"))?;

    emit_x86_rex(bytes, true, dst, 0, 5);
    bytes.push(0x8B);
    emit_x86_modrm_disp_rbp(bytes, dst, disp);
    Ok(())
}

fn emit_x86_64_store_stack(
    src: Reg,
    offset: i32,
    bytes: &mut Vec<u8>,
) -> Result<(), ObjectEmitError> {
    let src = x86_64_reg_code(src)?;
    let disp = 0_i32
        .checked_sub(offset)
        .ok_or_else(|| ObjectEmitError::new("x86_64 stack store offset overflow"))?;

    emit_x86_rex(bytes, true, src, 0, 5);
    bytes.push(0x89);
    emit_x86_modrm_disp_rbp(bytes, src, disp);
    Ok(())
}

fn emit_x86_modrm_disp_rbp(bytes: &mut Vec<u8>, reg_field: u8, disp: i32) {
    if (-128..=127).contains(&disp) {
        bytes.push(x86_modrm(0b01, reg_field, 5));
        bytes.push(disp as i8 as u8);
    } else {
        bytes.push(x86_modrm(0b10, reg_field, 5));
        bytes.extend_from_slice(&disp.to_le_bytes());
    }
}

fn x86_64_jcc_opcode(cond: Cond) -> u8 {
    match cond {
        Cond::Eq => 0x84,
        Cond::Ne => 0x85,
        Cond::Lt => 0x8C,
        Cond::Le => 0x8E,
        Cond::Gt => 0x8F,
        Cond::Ge => 0x8D,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_arm64_object_for_simple_main() {
        let function = MirFunction::with_instructions(
            "main",
            vec![
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::X0),
                    src: Operand::Imm(42),
                },
                MirInst::Ret,
            ],
        );

        let bytes = emit_object_file(&function, TargetArch::Arm64).expect("object emission failed");
        assert!(!bytes.is_empty());
        if cfg!(target_os = "macos") {
            assert_eq!(&bytes[0..4], &[0xcf, 0xfa, 0xed, 0xfe]);
        }
    }

    #[test]
    fn emits_x86_64_object_for_simple_main() {
        let function = MirFunction::with_instructions(
            "main",
            vec![
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RAX),
                    src: Operand::Imm(42),
                },
                MirInst::Ret,
            ],
        );

        let bytes = emit_object_file(&function, TargetArch::Amd64).expect("object emission failed");
        assert!(!bytes.is_empty());
        if cfg!(target_os = "macos") {
            assert_eq!(&bytes[0..4], &[0xcf, 0xfa, 0xed, 0xfe]);
        }
    }

    #[test]
    fn supports_arm64_call_relocations() {
        let function = MirFunction::with_instructions(
            "main",
            vec![
                MirInst::Call {
                    symbol: "callee".to_string(),
                },
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::X0),
                    src: Operand::Imm(0),
                },
                MirInst::Ret,
            ],
        );

        let bytes =
            emit_object_file(&function, TargetArch::Arm64).expect("call relocation should work");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn supports_x86_64_call_relocations() {
        let function = MirFunction::with_instructions(
            "main",
            vec![
                MirInst::Call {
                    symbol: "callee".to_string(),
                },
                MirInst::Mov {
                    dst: Reg::Phys(PhysReg::RAX),
                    src: Operand::Imm(0),
                },
                MirInst::Ret,
            ],
        );

        let bytes =
            emit_object_file(&function, TargetArch::Amd64).expect("call relocation should work");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn rejects_virtual_registers_in_object_emission() {
        let function = MirFunction::with_instructions(
            "main",
            vec![
                MirInst::Mov {
                    dst: Reg::VReg(0),
                    src: Operand::Imm(1),
                },
                MirInst::Ret,
            ],
        );

        let err = emit_object_file(&function, TargetArch::Arm64)
            .expect_err("expected physical-register validation error");
        assert!(err.to_string().contains("physical registers"));
    }
}
