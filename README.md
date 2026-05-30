# Ousia OS

Ousia OS is a Rust microkernel prototype. The current bootstrap path targets AArch64 first and uses a small host-side runner to build the kernel and launch it in QEMU.

## Repository layout

- `kernel/`: the kernel crate. It holds the bare-metal entry and core kernel logic.
- `ostd/`: the OS framework / kernel SDK layer. It currently contains early AArch64 boot helpers.
- `tools/qemu-runner/`: the host-side runner that builds the kernel and starts QEMU.
- `design/`: design notes and implementation drafts.

## Prerequisites

- Rust nightly toolchain
- `aarch64-unknown-none` target
- `llvm-tools-preview` component
- QEMU for AArch64 (`qemu-system-aarch64`)

Install the Rust pieces if needed:

```bash
rustup target add aarch64-unknown-none
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

## Lower-level checks

If you want to check pieces separately:

```bash
cargo check -p ostd --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo check -p kernel --target aarch64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
cargo test -p kernel
```

## Notes

- The first architecture target is AArch64.
- Host-side `build-std` is intentionally not enabled globally; it only belongs to the bare-metal kernel build.
- `tools/qemu-runner` is the preferred launch path for local development.
