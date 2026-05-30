use std::{
    path::PathBuf,
    process::{Command, ExitCode},
};

const TARGET: &str = "aarch64-unknown-none";

fn main() -> ExitCode {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("qemu-runner lives under tools/ in the workspace root")
        .to_path_buf();

    eprintln!("building aarch64 kernel...");
    if !run_command(Command::new("cargo").current_dir(&workspace_root).args([
        "build",
        "-p",
        "kernel",
        "--target",
        TARGET,
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=compiler-builtins-mem",
    ])) {
        return ExitCode::FAILURE;
    }

    let kernel_path = workspace_root
        .join("target")
        .join(TARGET)
        .join("debug")
        .join("kernel");

    eprintln!(
        "launching qemu-system-aarch64 with kernel {}",
        kernel_path.display()
    );
    let mut command = Command::new("qemu-system-aarch64");
    command
        .arg("-machine")
        .arg("virt")
        .arg("-cpu")
        .arg("cortex-a53")
        .arg("-m")
        .arg("1G")
        .arg("-smp")
        .arg("1")
        .arg("-kernel")
        .arg(&kernel_path)
        .arg("-serial")
        .arg("stdio")
        .arg("-monitor")
        .arg("none")
        .arg("-nographic")
        .arg("-display")
        .arg("none")
        .arg("-no-reboot")
        .arg("-no-shutdown");

    match command.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("failed to launch qemu-system-aarch64: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_command(command: &mut Command) -> bool {
    match command.status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("command exited with status {status}: {command:?}");
            false
        }
        Err(error) => {
            eprintln!("failed to run command {command:?}: {error}");
            false
        }
    }
}
