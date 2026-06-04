# Ousia OS

Ousia OS is a Rust microkernel prototype. AArch64 and amd64 are both first-class architecture targets. The current local run path tests AArch64 first through a small host-side runner.

## Repository layout

- `kernel/`: the kernel library crate. It owns architecture-neutral capability, IPC, object, scheduler, thread, invocation, and executor semantics.
- `kernel-bin/`: the bare-metal kernel binary crate. It owns `kernel_main`, panic handling, linker script selection, and OSTD boot wiring for QEMU-run images.
- `ostd/`: the OS framework / kernel SDK layer. It owns architecture bootstrap code, boot stacks, early console, and CPU helpers behind a common boot API.
- Some of the reusable console plumbing under `ostd/src/console/` is adapted from Asterinas MPL-2.0 code; the file headers carry the license notice.
- `tools/qemu-runner/`: an independent host-side control project that builds the kernel and starts QEMU.
- `design/`: design notes and implementation drafts.

## Prerequisites

- Rust nightly toolchain
- `aarch64-unknown-none` target
- `x86_64-unknown-none` target for amd64 checks
- `llvm-tools-preview` component
- QEMU for AArch64 (`qemu-system-aarch64`)

Install the Rust pieces if needed:

```bash
rustup target add aarch64-unknown-none
rustup target add x86_64-unknown-none
rustup component add llvm-tools-preview
```

## Build and run

From the workspace root:

```bash
cargo run --manifest-path tools/qemu-runner/Cargo.toml
```

That command will:

1. build `kernel-bin` for `aarch64-unknown-none`
2. launch `qemu-system-aarch64`
3. boot the kernel on the QEMU `virt` machine

The current kernel prints a short boot message through the AArch64 PL011 serial path and then waits forever.
This means `cargo run --manifest-path tools/qemu-runner/Cargo.toml` does not return by itself after a successful boot. QEMU owns the terminal until you quit it. In `-nographic` mode, press `Ctrl-A` and then `X` to exit QEMU.
If you stop QEMU with `Ctrl-C`, QEMU reports `terminating on signal 2`; that only means the host interrupted QEMU, not that the guest kernel failed.

For automated boot checks, use:

```bash
cargo run --manifest-path tools/qemu-runner/Cargo.toml -- --smoke
```

Smoke mode writes the guest serial stream to `target/qemu-aarch64.log`, waits for the boot marker, and then exits QEMU automatically. This is the path we will use for boot validation as the early console matures.

For the early AArch64 exception-vector diagnostic path, use:

```bash
cargo run --manifest-path tools/qemu-runner/Cargo.toml -- --exception-smoke
```

That mode builds `kernel-bin` with the `exception-smoke` feature, waits for the exception diagnostic marker, and then exits QEMU automatically.

AArch64 and amd64 are both first-class targets. The current runner only exercises AArch64. The amd64 path currently covers the OSTD-owned bare-metal bootstrap, early COM1 serial output, and halt loop so that architecture-specific code can compile and evolve behind the same `ostd` boundary without leaking into `kernel`.

## Lower-level checks

Plain `cargo check` validates the default workspace members `kernel` and `ostd` on the host target. `kernel-bin` is a bare-metal image crate, so validate it with an explicit `*-unknown-none` target instead of forcing host-only fallbacks into OSTD boot or heap modules.

If you want to check pieces separately:

```bash
cargo check
cargo check -p ostd --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p kernel --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p kernel-bin --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p ostd --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p kernel --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p kernel-bin --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo nextest run -p kernel
cargo nextest run -p ostd
```

## Editor analysis

The root workspace contains the architecture-neutral `kernel` library, the bare-metal `kernel-bin` image crate, and `ostd`. Default Cargo members are limited to host-checkable library crates, while `kernel-bin` remains a workspace member for explicit bare-metal builds. The workspace VS Code settings make rust-analyzer analyze bare-metal crates through the `aarch64-unknown-none` target with `core` and `alloc` built from source. This keeps `#[cfg(target_os = "none")]` modules visible to LSP while editing `kernel-bin` and `ostd` code.

`tools/qemu-runner` is intentionally outside the root workspace. Validate it with `cargo check --manifest-path tools/qemu-runner/Cargo.toml` when changing runner code rather than making bare-metal modules carry host-only fallback implementations for editor analysis.

## Notes

- AArch64 and amd64 are first-class architecture targets.
- The current QEMU runner tests AArch64 first; amd64 is validated through bare-metal compilation checks for now.
- Host-side `build-std` is intentionally not enabled globally; it only belongs to the bare-metal kernel build.
- `tools/qemu-runner` is an independent host control project and the preferred launch path for local development.
- The AArch64 direct-boot path follows the same boundary as seL4/rust-sel4 and Asterinas-style tooling: the runner owns QEMU machine and serial wiring, `ostd` owns early CPU state and device MMIO, `kernel-bin` owns bare-metal entry wiring, and `kernel` stays architecture-neutral.
- Before entering Rust on AArch64, `ostd` enables FP/SIMD access for the current exception level. This is required because Rust debug code can legally emit FP/SIMD instructions under the target ABI; later kernel FPU ownership and lazy context-switch policy must evolve toward the seL4-style model.
