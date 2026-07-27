# Backend Support Matrix

<!-- AUTO-GENERATED from the backend declarations; do not edit by hand.
     Regenerate: cargo xtask update-support-docs -->

The spec harness only tolerates test skips attributable to a feature that is
not `Supported` here ([ADR-8](adr/8-latest-testsuite-support-matrix.md)); an
unattributable failure is treated as a bug. Flipping a feature to supported
turns its remaining skips into hard failures until the tests pass.

## Features

The wasm 1.0 features a backend can meaningfully differ on ([ADR-25](adr/25-retire-support-tiers.md));
every other `Feature` variant is rejected by the core for every backend ([ADR-24](adr/24-01-scope-reset.md)).

| Feature | ruby | bash | python | go | java |
| --- | --- | --- | --- | --- | --- |
| Imported globals (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported memories (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported tables (wasm 1.0) | ✅ | ❌ | ✅ | ✅ | ✅ |
| Multiple tables | ✅ | ✅ | ✅ | ✅ | ✅ |
| Bulk table ops / passive element segments | ✅ | ❌ | ✅ | ✅ | ✅ |
| Floating-point (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ |

## WASI preview 1

Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS
stub ([ADR-7](adr/7-import-providers.md), bash conventions in
[ADR-12](adr/12-bash-wasi.md)). `—` marks the out-of-scope surface (sockets,
`proc_raise`) no toolchain output exercises (ADR-25).

| Function | ruby | bash | python | go | java |
| --- | --- | --- | --- | --- | --- |
| args_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| args_sizes_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| environ_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| environ_sizes_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| clock_res_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| clock_time_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_advise | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_allocate | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_close | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_datasync | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_fdstat_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_fdstat_set_flags | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_fdstat_set_rights | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_filestat_get | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_filestat_set_size | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_filestat_set_times | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_pread | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_prestat_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_prestat_dir_name | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_pwrite | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_read | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_readdir | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_renumber | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_seek | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_sync | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| fd_tell | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_write | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_create_directory | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| path_filestat_get | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| path_filestat_set_times | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_link | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_open | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| path_readlink | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_remove_directory | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| path_rename | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| path_symlink | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_unlink_file | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| poll_oneoff | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ |
| proc_exit | ✅ | ✅ | ✅ | ✅ | ✅ |
| proc_raise | — | — | — | — | — |
| random_get | ✅ | ✅ | ✅ | ✅ | ✅ |
| sched_yield | ✅ | ✅ | ✅ | ✅ | ✅ |
| sock_accept | — | — | — | — | — |
| sock_recv | — | — | — | — | — |
| sock_send | — | — | — | — | — |
| sock_shutdown | — | — | — | — | — |
