# Windows signal-hook compile fix

## Status

Completed.

## Changes

- Guarded `signal_hook::consts::signal::{SIGINT, SIGTERM}` and `signal_hook::iterator::{Handle, Signals}` imports with `#[cfg(not(windows))]`.
- Moved Unix signal listener setup into `start_publish_signal_listener`, also guarded with `#[cfg(not(windows))]`.
- Kept publish error cleanup shared across all platforms, so prepared deployments are still cancelled on upload/activation failures on Windows and Unix.
- Left Windows OS-level SIGINT/SIGTERM listener setup disabled because `signal_hook::iterator` is not available on Windows.

## Verification

- `cargo fmt --check`: passed.
- `cargo test`: passed, including publish cancellation tests.
- `cargo check --release`: passed.
- `rustup target add x86_64-pc-windows-msvc`: installed the Windows standard library target.
- `cargo check --target x86_64-pc-windows-msvc --release`: attempted, but local macOS cross-check failed before compiling `cfsurge` because `ring` could not compile C code for the Windows MSVC target without a Windows C toolchain/headers (`assert.h` not found).

## Notes

The CI Windows build should exercise the guarded code path on a real Windows runner where the MSVC toolchain is available. The normal cancellation path does not depend on signal listener setup and remains platform-neutral.
