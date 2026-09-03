---
name: clap-windows-stack
description: Debug pdfce-cli binary stack-overflows on ANY invocation when the clap command tree grows; fix = run CLI on a 16MB worker thread
metadata:
  type: reference
---

Adding subcommands to `pdfce-cli`'s clap `Command` enum can make the **debug**
binary stack-overflow on **every** invocation (`--help`, `inspect`, anything)
with a silent `thread 'main' has overflowed its stack` and exit code 0. The
release binary is unaffected.

**Why:** clap runs `Command::debug_assert()` inside `get_matches` only under
`#[cfg(debug_assertions)]`. Its recursion over the command tree, with
unoptimized (huge) debug frames, exceeds Windows/MSVC's small default
**main-thread** stack (~1 MB) once the tree is big enough. A spawned thread
(default larger stack) or a release build fits fine — which is why
`debug_assert()` on a 64 MB probe thread passes while the real binary crashes.

**Symptom that misleads:** the CLI integration tests (`crates/pdfce-cli/tests/edit_commands.rs`)
spawn the debug binary and `u8::try_from(exit_code).unwrap()` panics with
`TryFromIntError(NegOverflow)` because the crash exit code is negative-as-i32.
This looks like a logic bug in every test; it is actually the binary crashing
at clap parse before running any command.

**Fix (in place in `main.rs`):** `fn main()` spawns `run()` on a
`std::thread::Builder::new().stack_size(16 << 20)` worker and joins it, falling
back to the main thread if spawn fails. Standard portable Windows workaround;
no-op elsewhere. Do NOT chase it as a clap-config error — the command tree is
valid (`debug_assert` passes on a big stack).

Related: [[project_pass71_forms]]
