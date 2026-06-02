use clap::Parser;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode},
    thread,
    time::{Duration, Instant},
};

const TARGET: &str = "aarch64-unknown-none";
const QEMU_CPUS: &str = "2";
const BOOT_MARKER: &str = "Ousia kernel booted on aarch64";
const EXCEPTION_MARKER: &str = "Ousia AArch64 exception";
const SMOKE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Parser)]
#[command(about = "Build the AArch64 kernel and launch it with QEMU")]
struct Args {
    #[arg(long, conflicts_with = "exception_smoke")]
    smoke: bool,

    #[arg(long, conflicts_with = "smoke")]
    exception_smoke: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("qemu-runner lives under tools/ in the workspace root")
        .to_path_buf();

    eprintln!("building aarch64 kernel...");
    let mut build_command = Command::new("cargo");
    build_command
        .current_dir(&workspace_root)
        .args(["build", "-p", "kernel-bin", "--target", TARGET]);
    if args.exception_smoke {
        build_command.args(["--features", "exception-smoke"]);
    }
    build_command.args([
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=compiler-builtins-mem",
    ]);

    if !run_command(&mut build_command) {
        return ExitCode::FAILURE;
    }

    let kernel_path = workspace_root
        .join("target")
        .join(TARGET)
        .join("debug")
        .join("kernel");

    if args.smoke || args.exception_smoke {
        let marker = if args.exception_smoke {
            EXCEPTION_MARKER
        } else {
            BOOT_MARKER
        };
        return run_smoke(&workspace_root, &kernel_path, marker);
    }

    eprintln!(
        "launching qemu-system-aarch64 with kernel {}",
        kernel_path.display()
    );
    let mut command = qemu_command(&kernel_path);
    command.arg("-serial").arg("stdio");

    match command.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("failed to launch qemu-system-aarch64: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_smoke(workspace_root: &Path, kernel_path: &Path, marker: &str) -> ExitCode {
    let log_path = workspace_root.join("target").join("qemu-aarch64.log");
    if let Err(error) = fs::write(&log_path, "") {
        eprintln!("failed to clear {}: {error}", log_path.display());
        return ExitCode::FAILURE;
    }

    eprintln!(
        "launching qemu-system-aarch64 smoke test; serial log: {}",
        log_path.display()
    );

    let mut command = qemu_command(kernel_path);
    command
        .arg("-chardev")
        .arg(format!("file,id=serial0,path={}", log_path.display()))
        .arg("-serial")
        .arg("chardev:serial0");

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("failed to launch qemu-system-aarch64: {error}");
            return ExitCode::FAILURE;
        }
    };

    let deadline = Instant::now() + SMOKE_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            eprintln!("qemu exited before smoke marker with status {status}");
            return ExitCode::FAILURE;
        }

        if log_contains(&log_path, marker) {
            stop_qemu(&mut child);
            eprintln!("smoke test passed: found `{marker}`");
            return ExitCode::SUCCESS;
        }

        thread::sleep(Duration::from_millis(100));
    }

    stop_qemu(&mut child);
    eprintln!(
        "smoke test timed out after {}s; expected `{marker}` in {}",
        SMOKE_TIMEOUT.as_secs(),
        log_path.display()
    );
    ExitCode::FAILURE
}

fn qemu_command(kernel_path: &Path) -> Command {
    let mut command = Command::new("qemu-system-aarch64");
    command
        .arg("-machine")
        .arg("virt")
        .arg("-cpu")
        .arg("cortex-a53")
        .arg("-m")
        .arg("1G")
        .arg("-smp")
        .arg(QEMU_CPUS)
        .arg("-kernel")
        .arg(kernel_path)
        .arg("-monitor")
        .arg("none")
        .arg("-display")
        .arg("none")
        .arg("-no-reboot")
        .arg("-no-shutdown");
    command
}

fn log_contains(path: &Path, marker: &str) -> bool {
    fs::read_to_string(path).is_ok_and(|log| log.contains(marker))
}

fn stop_qemu(child: &mut Child) {
    if let Err(error) = child.kill() {
        eprintln!("failed to stop qemu-system-aarch64: {error}");
    }
    let _ = child.wait();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_command_uses_multicore_topology() {
        let command = qemu_command(Path::new("target/aarch64-unknown-none/debug/kernel"));
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();

        let smp = args
            .windows(2)
            .find(|window| window[0] == "-smp")
            .expect("qemu command should include -smp");
        assert_eq!(smp[1], QEMU_CPUS);
    }
}
