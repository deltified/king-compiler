use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use king_compiler::{
    compile_source_to_ir, emit_assembly, linear_scan_allocate, lower_il_to_mir, Function,
    TargetArch,
};

fn main() {
    if let Err(err) = try_main() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os();
    let program_name = args
        .next()
        .unwrap_or_else(|| std::ffi::OsString::from("king-compiler"));

    let Some(input_arg) = args.next() else {
        print_usage(&program_name);
        return Err(invalid_usage_error().into());
    };

    if args.next().is_some() {
        print_usage(&program_name);
        return Err(invalid_usage_error().into());
    }

    if input_arg == OsStr::new("-h") || input_arg == OsStr::new("--help") {
        print_usage(&program_name);
        return Ok(());
    }

    let input_path = PathBuf::from(input_arg);
    let source = fs::read_to_string(&input_path)?;
    let functions = compile_source_to_ir(&source)?;

    let x86_64_path = write_target_assembly(&input_path, &functions, TargetArch::X86_64)?;
    let arm64_path = write_target_assembly(&input_path, &functions, TargetArch::Arm64)?;

    println!(
        "Wrote {} and {}",
        x86_64_path.display(),
        arm64_path.display()
    );

    Ok(())
}

fn write_target_assembly(
    input_path: &Path,
    functions: &[Function],
    target: TargetArch,
) -> Result<PathBuf, Box<dyn Error>> {
    let assembly = emit_program_assembly(functions, target)?;
    let output_path = match target {
        TargetArch::Arm64 => input_path.with_extension("arm64.s"),
        TargetArch::Amd64 | TargetArch::X86_64 => input_path.with_extension("x86_64.s"),
    };

    fs::write(&output_path, assembly)?;
    Ok(output_path)
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

fn print_usage(program_name: &OsStr) {
    let program_name = program_name.to_string_lossy();
    eprintln!("Usage: {program_name} <source.king>");
    eprintln!("Compiles a mini-language source file to x86_64 and arm64 assembly.");
}

fn invalid_usage_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "expected exactly one source file path",
    )
}
