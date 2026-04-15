use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use king_compiler::{
    compile_source_to_ir, emit_assembly, linear_scan_allocate, lower_il_to_mir, Function,
    TargetArch,
};

#[test]
fn cli_compiles_king_file_to_both_architectures() -> Result<(), Box<dyn Error>> {
    let workdir = create_workdir()?;
    let source_path = workdir.join("sample.king");
    let source = r#"
        fn add2(a, b) = a + b;
        fn main() = add2(40, 2);
    "#;

    fs::write(&source_path, source)?;

    let functions = compile_source_to_ir(source)?;
    let expected_x86_64 = emit_program_assembly(&functions, TargetArch::X86_64)?;
    let expected_arm64 = emit_program_assembly(&functions, TargetArch::Arm64)?;

    let output = Command::new(env!("CARGO_BIN_EXE_king-compiler"))
        .arg(&source_path)
        .current_dir(&workdir)
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "compiler failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let x86_64_path = source_path.with_extension("x86_64.s");
    let arm64_path: PathBuf = source_path.with_extension("arm64.s");

    assert_eq!(fs::read_to_string(&x86_64_path)?, expected_x86_64);
    assert_eq!(fs::read_to_string(&arm64_path)?, expected_arm64);

    Ok(())
}

fn emit_program_assembly(
    functions: &[Function],
    target: TargetArch,
) -> Result<String, Box<dyn Error>> {
    let mut chunks = Vec::with_capacity(functions.len());

    for function in functions {
        let mir = lower_il_to_mir(function, target)?;
        let allocated = linear_scan_allocate(&mir, target)?;
        let assembly = emit_assembly(&allocated.function, target)?;
        chunks.push(assembly.trim_end_matches('\n').to_string());
    }

    let mut output = chunks.join("\n\n");
    if !output.is_empty() {
        output.push('\n');
    }

    Ok(output)
}

fn create_workdir() -> Result<PathBuf, Box<dyn Error>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "king_compiler_cli_{}_{}",
        std::process::id(),
        now
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}
