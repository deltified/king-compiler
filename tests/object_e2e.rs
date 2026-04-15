#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod macos_arm64_object_e2e {
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use king_compiler::ir::{Function, IrBuilder, Type};
    use king_compiler::lowering::lower_il_to_mir;
    use king_compiler::mir::TargetArch;
    use king_compiler::object_emit::emit_object_file;
    use king_compiler::regalloc::linear_scan_allocate;

    #[test]
    fn phase6_object_e2e_main_42_arm64() -> Result<(), Box<dyn Error>> {
        let workdir = create_workdir()?;
        let function = build_main_42()?;
        let object_path =
            lower_allocate_emit_object(&workdir, "main_42", &function, TargetArch::Arm64)?;

        let binary_path = workdir.join("main_42_bin");
        link_arm64_objects(&[&object_path], &binary_path)?;

        assert_eq!(run_arm64_binary(&binary_path)?, 42);
        Ok(())
    }

    #[test]
    fn phase6_object_e2e_extended_ops_arm64() -> Result<(), Box<dyn Error>> {
        let workdir = create_workdir()?;
        let function = build_extended_main()?;
        let object_path =
            lower_allocate_emit_object(&workdir, "extended", &function, TargetArch::Arm64)?;

        let binary_path = workdir.join("extended_bin");
        link_arm64_objects(&[&object_path], &binary_path)?;

        assert_eq!(run_arm64_binary(&binary_path)?, 10);
        Ok(())
    }

    #[test]
    fn phase6_object_e2e_call_relocation_arm64() -> Result<(), Box<dyn Error>> {
        let workdir = create_workdir()?;

        let add2 = build_add2()?;
        let main = build_call_main()?;
        let add2_obj = lower_allocate_emit_object(&workdir, "add2", &add2, TargetArch::Arm64)?;
        let main_obj = lower_allocate_emit_object(&workdir, "main_call", &main, TargetArch::Arm64)?;

        let binary_path = workdir.join("call_reloc_arm64_bin");
        link_arm64_objects(&[&add2_obj, &main_obj], &binary_path)?;

        assert_eq!(run_arm64_binary(&binary_path)?, 42);
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

    fn build_call_main() -> Result<Function, Box<dyn Error>> {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder.position_at_end(entry)?;
        let forty = builder.build_const_i32(40)?;
        let two = builder.build_const_i32(2)?;
        let sum = builder
            .build_call(
                Type::I32,
                "add2",
                vec![(Type::I32, forty), (Type::I32, two)],
            )?
            .ok_or("add2 call should return a value")?;
        builder.build_ret(Some(sum))?;
        Ok(builder.finish())
    }

    fn lower_allocate_emit_object(
        workdir: &Path,
        stem: &str,
        function: &Function,
        target: TargetArch,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let mir = lower_il_to_mir(function, target)?;
        let allocated = linear_scan_allocate(&mir, target)?;
        let object_bytes = emit_object_file(&allocated.function, target)?;

        let object_path = workdir.join(format!("{stem}.o"));
        fs::write(&object_path, object_bytes)?;
        Ok(object_path)
    }

    fn create_workdir() -> Result<PathBuf, Box<dyn Error>> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "king_compiler_phase6_{}_{}",
            std::process::id(),
            now
        ));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn link_arm64_objects(objects: &[&Path], output: &Path) -> Result<(), Box<dyn Error>> {
        let mut cmd = Command::new("cc");
        cmd.arg("-target").arg("arm64-apple-macos13");
        for object in objects {
            cmd.arg(object);
        }
        cmd.arg("-o").arg(output);

        let result = cmd.output()?;
        if !result.status.success() {
            return Err(format!("cc failed: {}", String::from_utf8_lossy(&result.stderr)).into());
        }

        Ok(())
    }

    fn run_arm64_binary(binary: &Path) -> Result<i32, Box<dyn Error>> {
        let status = Command::new(binary).status()?;
        status
            .code()
            .ok_or_else(|| "process terminated by signal".into())
    }
}

#[cfg(target_os = "macos")]
mod macos_x86_64_object_e2e {
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use king_compiler::ir::{Function, IrBuilder, Type};
    use king_compiler::lowering::lower_il_to_mir;
    use king_compiler::mir::TargetArch;
    use king_compiler::object_emit::emit_object_file;
    use king_compiler::regalloc::linear_scan_allocate;

    #[test]
    fn phase6_object_e2e_main_42_x86_64() -> Result<(), Box<dyn Error>> {
        let workdir = create_workdir()?;
        let function = build_main_42()?;
        let object_path =
            lower_allocate_emit_object(&workdir, "main_42", &function, TargetArch::Amd64)?;

        let binary_path = workdir.join("main_42_x86_64_bin");
        link_x86_64_objects(&[&object_path], &binary_path)?;

        assert_eq!(run_x86_64_binary(&binary_path)?, 42);
        Ok(())
    }

    #[test]
    fn phase6_object_e2e_extended_ops_x86_64() -> Result<(), Box<dyn Error>> {
        let workdir = create_workdir()?;
        let function = build_extended_main()?;
        let object_path =
            lower_allocate_emit_object(&workdir, "extended", &function, TargetArch::Amd64)?;

        let binary_path = workdir.join("extended_x86_64_bin");
        link_x86_64_objects(&[&object_path], &binary_path)?;

        assert_eq!(run_x86_64_binary(&binary_path)?, 10);
        Ok(())
    }

    #[test]
    fn phase6_object_e2e_call_relocation_x86_64() -> Result<(), Box<dyn Error>> {
        let workdir = create_workdir()?;

        let add2 = build_add2()?;
        let main = build_call_main()?;
        let add2_obj = lower_allocate_emit_object(&workdir, "add2", &add2, TargetArch::Amd64)?;
        let main_obj = lower_allocate_emit_object(&workdir, "main_call", &main, TargetArch::Amd64)?;

        let binary_path = workdir.join("call_reloc_x86_64_bin");
        link_x86_64_objects(&[&add2_obj, &main_obj], &binary_path)?;

        assert_eq!(run_x86_64_binary(&binary_path)?, 42);
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

    fn build_call_main() -> Result<Function, Box<dyn Error>> {
        let mut builder = IrBuilder::new("main", Type::I32);
        let entry = builder.create_block("entry");
        builder.position_at_end(entry)?;
        let forty = builder.build_const_i32(40)?;
        let two = builder.build_const_i32(2)?;
        let sum = builder
            .build_call(
                Type::I32,
                "add2",
                vec![(Type::I32, forty), (Type::I32, two)],
            )?
            .ok_or("add2 call should return a value")?;
        builder.build_ret(Some(sum))?;
        Ok(builder.finish())
    }

    fn lower_allocate_emit_object(
        workdir: &Path,
        stem: &str,
        function: &Function,
        target: TargetArch,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let mir = lower_il_to_mir(function, target)?;
        let allocated = linear_scan_allocate(&mir, target)?;
        let object_bytes = emit_object_file(&allocated.function, target)?;

        let object_path = workdir.join(format!("{stem}.o"));
        fs::write(&object_path, object_bytes)?;
        Ok(object_path)
    }

    fn create_workdir() -> Result<PathBuf, Box<dyn Error>> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "king_compiler_phase6_x86_{}_{}",
            std::process::id(),
            now
        ));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn link_x86_64_objects(objects: &[&Path], output: &Path) -> Result<(), Box<dyn Error>> {
        let mut cmd = Command::new("cc");
        cmd.arg("-target").arg("x86_64-apple-macos13");
        for object in objects {
            cmd.arg(object);
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
