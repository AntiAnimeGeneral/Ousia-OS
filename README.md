# Ousia OS

Ousia OS is a Rust microkernel prototype. AArch64 and amd64 are both first-class architecture targets. The current local run path tests AArch64 first through a small host-side runner.

## Repository layout

- `kernel/`: the kernel crate. It holds the bare-metal entry and core kernel logic. Architecture bootstrap code lives under `kernel/src/boot/`.
- `ostd/`: the OS framework / kernel SDK layer. It contains architecture-specific early console and CPU helpers behind a common boot API.
- `tools/qemu-runner/`: the host-side runner that builds the kernel and starts QEMU.
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
cargo run -p qemu-runner
```

That command will:

1. build `kernel` for `aarch64-unknown-none`
2. launch `qemu-system-aarch64`
3. boot the kernel on the QEMU `virt` machine

The current kernel prints a short boot message through the AArch64 PL011 serial path and then waits forever.
This means `cargo run -p qemu-runner` does not return by itself after a successful boot. QEMU owns the terminal until you quit it. In `-nographic` mode, press `Ctrl-A` and then `X` to exit QEMU.

AArch64 and amd64 are both first-class targets. The current runner only exercises AArch64. The amd64 path currently covers the bare-metal entry, early COM1 serial output, and halt loop so that architecture-specific kernel code can compile and evolve behind the same `ostd` boundary.

## Lower-level checks

If you want to check pieces separately:

```bash
cargo check -p ostd --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p kernel --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p ostd --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p kernel --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo test -p kernel
```

## Notes

- AArch64 and amd64 are first-class architecture targets.
- The current QEMU runner tests AArch64 first; amd64 is validated through bare-metal compilation checks for now.
- Host-side `build-std` is intentionally not enabled globally; it only belongs to the bare-metal kernel build.
- `tools/qemu-runner` is the preferred launch path for local development.
