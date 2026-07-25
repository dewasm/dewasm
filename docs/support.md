# Backend Support Matrix

<!-- AUTO-GENERATED from the backend declarations; do not edit by hand.
     Regenerate: DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs -->

The spec harness only tolerates test skips attributable to a feature that is
not `Supported` here ([ADR-8](adr/8-latest-testsuite-support-matrix.md)); an
unattributable failure is treated as a bug. Flipping a feature to supported
turns its remaining skips into hard failures until the tests pass.

## Tiers

Zig-style support tiers specialized to wasm 1.0 + WASI preview 1
([ADR-23](adr/23-backend-support-tiers.md) has the requirement
checklists); post-1.0 proposals and the component model are the
extension badges below and never affect a tier. `Current` is derived
from the declarations on this page.

- **Tier 1 (Full)** — wasm 1.0 + WASI p1 handled completely (all p1 functions short of the out-of-scope socket surface, empty wasm-1.0 failure ledger)
- **Tier 2 (Production)** — real-world p1 binaries from the major toolchains convert and run at production quality (full wasm 1.0 imports, WASI filesystem); the standard goal for new backends
- **Tier 3 (Core)** — self-contained wasm 1.0 CLI binaries (function imports only, WASI core) convert and run; at least one real app verified end to end
- **Tier 4 (Experimental)** — under development, no guarantees; wired into the spec harness but outside the default gate

| Backend | Current | Target |
| --- | --- | --- |
| ruby | Tier 2 (Production) | Tier 1 (Full) |
| bash | Tier 3 (Core) | Tier 3 (Core) |

## Baseline

- **ruby**: Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, sign-extension, saturating float-to-int, multi-value, and the memory half of bulk memory (memory.copy/fill/init, passive data segments)
- **bash**: Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, sign-extension, saturating float-to-int, multi-value, and the memory half of bulk memory; f32/f64 run on the pure-Bash softfloat (ADR-5/ADR-13); requires bash >= 5

## Wasm 1.0 features (tiered)

| Feature | Tier | ruby | bash |
| --- | --- | --- | --- |
| Imported globals (wasm 1.0) | Tier 2 (Production) | ✅ | ❌ |
| Imported memories (wasm 1.0) | Tier 2 (Production) | ✅ | ❌ |
| Imported tables (wasm 1.0) | Tier 2 (Production) | ✅ | ❌ |
| Floating-point (wasm 1.0) | Tier 3 (Core) | ✅ | ✅ |

## Extensions (badges)

Per-backend opt-ins, orthogonal to the tiers ([ADR-23](adr/23-backend-support-tiers.md)).

| Feature | ruby | bash |
| --- | --- | --- |
| Multiple tables | ✅ | ❌ |
| Bulk table ops / passive element segments | ✅ | ❌ |
| Reference types | ❌ | ❌ |
| Typed function references | ❌ | ❌ |
| Garbage collection | ❌ | ❌ |
| Multiple memories | ❌ | ❌ |
| Extended constant expressions | ❌ | ❌ |
| SIMD | ❌ | ❌ |
| Relaxed SIMD | ❌ | ❌ |
| Threads and atomics | ❌ | ❌ |
| Tail calls | ❌ | ❌ |
| 64-bit memory | ❌ | ❌ |
| Exception handling | ❌ | ❌ |
| Wide arithmetic | ❌ | ❌ |
| Custom page sizes | ❌ | ❌ |
| Component model / WASI preview 2 | ❌ | ❌ |

## WASI preview 1

Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS
stub ([ADR-7](adr/7-import-providers.md), bash conventions in
[ADR-12](adr/12-bash-wasi.md)). The tier is the one that requires the
function; `—` marks the out-of-scope surface no tier requires (ADR-23).

| Function | Tier | ruby | bash |
| --- | --- | --- | --- |
| args_get | Tier 3 (Core) | ✅ | ✅ |
| args_sizes_get | Tier 3 (Core) | ✅ | ✅ |
| environ_get | Tier 3 (Core) | ✅ | ✅ |
| environ_sizes_get | Tier 3 (Core) | ✅ | ✅ |
| clock_res_get | Tier 3 (Core) | ✅ | ✅ |
| clock_time_get | Tier 3 (Core) | ✅ | ✅ |
| fd_advise | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_allocate | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_close | Tier 3 (Core) | ✅ | ✅ |
| fd_datasync | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_fdstat_get | Tier 3 (Core) | ✅ | ✅ |
| fd_fdstat_set_flags | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_fdstat_set_rights | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_filestat_get | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_filestat_set_size | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_filestat_set_times | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_pread | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_prestat_get | Tier 3 (Core) | ✅ | ✅ |
| fd_prestat_dir_name | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_pwrite | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_read | Tier 3 (Core) | ✅ | ✅ |
| fd_readdir | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_renumber | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_seek | Tier 3 (Core) | ✅ | ✅ |
| fd_sync | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| fd_tell | Tier 3 (Core) | ✅ | ✅ |
| fd_write | Tier 3 (Core) | ✅ | ✅ |
| path_create_directory | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| path_filestat_get | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| path_filestat_set_times | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_link | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_open | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| path_readlink | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_remove_directory | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| path_rename | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| path_symlink | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_unlink_file | Tier 2 (Production) | ✅ | ❌ (ENOSYS) |
| poll_oneoff | Tier 1 (Full) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| proc_exit | Tier 3 (Core) | ✅ | ✅ |
| proc_raise | — | ❌ (ENOSYS) | ❌ (ENOSYS) |
| random_get | Tier 3 (Core) | ✅ | ✅ |
| sched_yield | Tier 3 (Core) | ✅ | ✅ |
| sock_accept | — | ❌ (ENOSYS) | ❌ (ENOSYS) |
| sock_recv | — | ❌ (ENOSYS) | ❌ (ENOSYS) |
| sock_send | — | ❌ (ENOSYS) | ❌ (ENOSYS) |
| sock_shutdown | — | ❌ (ENOSYS) | ❌ (ENOSYS) |
