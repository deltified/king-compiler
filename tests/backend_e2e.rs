#[cfg(target_os = "macos")]
mod macos_e2e {
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use king_compiler::ir::{Function, IrBuilder, Type, build_factorial_il, run_phase5_pipeline};
    use king_compiler::lowering::lower_il_to_mir;
    use king_compiler::mir::{TargetArch, emit_assembly};
    use king_compiler::regalloc::linear_scan_allocate;

    #[test]
    fn backend_e2e_macos_x86_64_paths() -> Result<(), Box<dyn Error>> {
        let workdir = create_workdir()?;

        let main_fn = build_main_42()?;
        let extended_fn = build_extended_main()?;
        let call_main_fn = build_call_main()?;
        let identity_fn = build_identity()?;
        let add2_fn = build_add2()?;
        let factorial_fn = build_factorial_il()?;
        let phase5_factorial_fn = run_phase5_pipeline(factorial_fn.clone());

        let asm_main = emit_amd64_asm(&main_fn)?;
        let asm_extended = emit_amd64_asm(&extended_fn)?;
        let asm_call_main = emit_amd64_asm(&call_main_fn)?;
        let asm_identity = emit_amd64_asm(&identity_fn)?;
        let asm_add2 = emit_amd64_asm(&add2_fn)?;
        let asm_factorial = emit_amd64_asm(&factorial_fn)?;
        let asm_phase5_factorial = emit_amd64_asm(&phase5_factorial_fn)?;

        let main_asm_path = write_file(&workdir, "main_42.s", &asm_main)?;
        let extended_asm_path = write_file(&workdir, "extended_main.s", &asm_extended)?;
        let call_main_asm_path = write_file(&workdir, "call_main.s", &asm_call_main)?;
        let identity_asm_path = write_file(&workdir, "identity.s", &asm_identity)?;
        let add2_asm_path = write_file(&workdir, "add2.s", &asm_add2)?;
        let factorial_asm_path = write_file(&workdir, "factorial.s", &asm_factorial)?;
        let phase5_factorial_asm_path =
            write_file(&workdir, "phase5_factorial.s", &asm_phase5_factorial)?;

        let pack2_helper = write_file(
            &workdir,
            "pack2_helper.c",
            "int pack2(int a, int b) { return a * 10 + b; }\n",
        )?;
        let identity_harness = write_file(
            &workdir,
            "identity_harness.c",
            "extern int identity(int n);\nint main(void) { return identity(42); }\n",
        )?;
        let add2_harness = write_file(
            &workdir,
            "add2_harness.c",
            "extern int add2(int a, int b);\nint main(void) { return add2(40, 2); }\n",
        )?;
        let factorial_harness = write_file(
            &workdir,
            "factorial_harness.c",
            "extern int factorial(int n);\nint main(void) { return factorial(5); }\n",
        )?;

        let main_bin = workdir.join("main_42_bin");
        let extended_bin = workdir.join("extended_bin");
        let call_main_bin = workdir.join("call_main_bin");
        let identity_bin = workdir.join("identity_bin");
        let add2_bin = workdir.join("add2_bin");
        let factorial_bin = workdir.join("factorial_bin");
        let phase5_factorial_bin = workdir.join("phase5_factorial_bin");

        compile_x86_64_macos(&[&main_asm_path], &main_bin)?;
        compile_x86_64_macos(&[&extended_asm_path], &extended_bin)?;
        compile_x86_64_macos(&[&call_main_asm_path, &pack2_helper], &call_main_bin)?;
        compile_x86_64_macos(&[&identity_asm_path, &identity_harness], &identity_bin)?;
        compile_x86_64_macos(&[&add2_asm_path, &add2_harness], &add2_bin)?;
        compile_x86_64_macos(&[&factorial_asm_path, &factorial_harness], &factorial_bin)?;
        compile_x86_64_macos(
            &[&phase5_factorial_asm_path, &factorial_harness],
            &phase5_factorial_bin,
        )?;

        assert_eq!(
            run_x86_64_binary(&main_bin)?,
            42,
            "main(42) exit code mismatch"
        );
        assert_eq!(
            run_x86_64_binary(&extended_bin)?,
            10,
            "extended main exit code mismatch"
        );
        assert_eq!(
            run_x86_64_binary(&call_main_bin)?,
            42,
            "call main exit code mismatch"
        );
        assert_eq!(
            run_x86_64_binary(&identity_bin)?,
            42,
            "identity ABI exit code mismatch"
        );
        assert_eq!(
            run_x86_64_binary(&add2_bin)?,
            42,
            "add2 ABI exit code mismatch"
        );
        assert_eq!(
            run_x86_64_binary(&factorial_bin)?,
            120,
            "phase4 factorial exit code mismatch"
        );
        assert_eq!(
            run_x86_64_binary(&phase5_factorial_bin)?,
            120,
            "phase5 factorial exit code mismatch"
        );

        Ok(())
    }

    fn build_main_42() -> Result<Function, Box<dyn Error>> {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder.position_at_end(entry)?;
        let value_42 = builder.build_const_i32(42)?;
        builder.build_ret(Some(value_42))?;
        Ok(builder.finish())
    }

    fn build_extended_main() -> Result<Function, Box<dyn Error>> {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder.position_at_end(entry)?;

        let ptr = builder.build_alloca(Type::I32)?;
        let eighty_four = builder.build_const_i32(84)?;
        builder.build_store(Type::I32, eighty_four, ptr)?;
        let loaded = builder.build_load(Type::I32, ptr)?;
        let two = builder.build_const_i32(2)?;
        let divided = builder.build_sdiv(loaded, two)?;
        let mask = builder.build_const_i32(31)?;
        let masked = builder.build_and(divided, mask)?;
        builder.build_ret(Some(masked))?;

        Ok(builder.finish())
    }

    fn build_call_main() -> Result<Function, Box<dyn Error>> {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder.position_at_end(entry)?;

        let four = builder.build_const_i32(4)?;
        let two = builder.build_const_i32(2)?;
        let packed = builder
            .build_call(
                Type::I32,
                "pack2",
                vec![(Type::I32, four), (Type::I32, two)],
            )?
            .ok_or("pack2 should return a value")?;
        builder.build_ret(Some(packed))?;

        Ok(builder.finish())
    }

    fn build_identity() -> Result<Function, Box<dyn Error>> {
        let mut builder = IrBuilder::new("identity", Type::I32);
        let n = builder.add_param("n", Type::I32);
        let entry = builder.create_block("entry");
        builder.position_at_end(entry)?;
        builder.build_ret(Some(n))?;
        Ok(builder.finish())
    }

    fn build_add2() -> Result<Function, Box<dyn Error>> {
        let mut builder = IrBuilder::new("add2", Type::I32);
        let a = builder.add_param("a", Type::I32);
        let b = builder.add_param("b", Type::I32);
        let entry = builder.create_block("entry");
        builder.position_at_end(entry)?;
        let sum = builder.build_add(a, b)?;
        builder.build_ret(Some(sum))?;
        Ok(builder.finish())
    }

    fn emit_amd64_asm(function: &Function) -> Result<String, Box<dyn Error>> {
        let mir = lower_il_to_mir(function, TargetArch::Amd64)?;
        let allocated = linear_scan_allocate(&mir, TargetArch::Amd64)?;
        Ok(emit_assembly(&allocated.function, TargetArch::Amd64)?)
    }

    fn create_workdir() -> Result<PathBuf, Box<dyn Error>> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir =
            std::env::temp_dir().join(format!("king_compiler_e2e_{}_{}", std::process::id(), now));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn write_file(base: &Path, name: &str, contents: &str) -> Result<PathBuf, Box<dyn Error>> {
        let path = base.join(name);
        fs::write(&path, contents)?;
        Ok(path)
    }

    fn compile_x86_64_macos(sources: &[&Path], output: &Path) -> Result<(), Box<dyn Error>> {
        let mut cmd = Command::new("cc");
        cmd.arg("-target").arg("x86_64-apple-macos13");
        for source in sources {
            cmd.arg(source);
        }
        cmd.arg("-o").arg(output);

        let result = cmd.output()?;
        if !result.status.success() {
            return Err(format!("cc failed: {}", String::from_utf8_lossy(&result.stderr)).into());
        }

        Ok(())
    }

    fn run_x86_64_binary(binary: &Path) -> Result<i32, Box<dyn Error>> {
        let status = if cfg!(target_arch = "aarch64") {
            Command::new("arch").arg("-x86_64").arg(binary).status()?
        } else {
            Command::new(binary).status()?
        };

        status
            .code()
            .ok_or_else(|| "process terminated by signal".into())
    }
}
