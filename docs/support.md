# Backend Support Matrix

<!-- AUTO-GENERATED from the backend declarations; do not edit by hand. Regenerate: cargo xtask update-support-docs -->

The spec harness only tolerates test skips attributable to a feature that is not `Supported` here ([ADR-8](adr/8-latest-testsuite-support-matrix.md)); an unattributable failure is treated as a bug. Flipping a feature to supported turns its remaining skips into hard failures until the tests pass.

## Features

The wasm 1.0 features a backend can meaningfully differ on ([ADR-25](adr/25-retire-support-tiers.md)); every other `Feature` variant is rejected by the core for every backend ([ADR-24](adr/24-01-scope-reset.md)).

| Feature | ruby | bash | python | perl | go | java |
| --- | --- | --- | --- | --- | --- | --- |
| Imported globals (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported memories (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported tables (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multiple tables | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Bulk table ops / passive element segments | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Floating-point (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

## WASI preview 1

Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS stub ([ADR-7](adr/7-import-providers.md), bash conventions in [ADR-12](adr/12-bash-wasi.md)). `—` marks the out-of-scope surface (sockets, `proc_raise`) no toolchain output exercises (ADR-25).

| Function | ruby | bash | python | perl | go | java |
| --- | --- | --- | --- | --- | --- | --- |
| args_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| args_sizes_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| environ_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| environ_sizes_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| clock_res_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| clock_time_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_advise | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_allocate | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_close | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_datasync | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_fdstat_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_fdstat_set_flags | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_fdstat_set_rights | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_filestat_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_filestat_set_size | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_filestat_set_times | ✅ | ❌ (ENOSYS) | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_pread | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_prestat_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_prestat_dir_name | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_pwrite | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_read | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_readdir | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_renumber | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_seek | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_sync | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_tell | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| fd_write | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_create_directory | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_filestat_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_filestat_set_times | ✅ | ❌ (ENOSYS) | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_link | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_open | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_readlink | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_remove_directory | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_rename | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_symlink | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| path_unlink_file | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| poll_oneoff | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| proc_exit | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| proc_raise | — | — | — | — | — | — |
| random_get | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| sched_yield | ✅ | ✅ | ✅ | ❌ (ENOSYS) | ✅ | ✅ |
| sock_accept | — | — | — | — | — | — |
| sock_recv | — | — | — | — | — | — |
| sock_send | — | — | — | — | — | — |
| sock_shutdown | — | — | — | — | — | — |
