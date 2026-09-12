# Backend Support Matrix

<!-- AUTO-GENERATED from the backend declarations; do not edit by hand. Regenerate: cargo xtask update-support-docs -->

The spec harness only tolerates test skips attributable to a feature that is not `Supported` here; an unattributable failure is treated as a bug. Flipping a feature to supported turns its remaining skips into hard failures until the tests pass.

## Features

The features a backend can meaningfully differ on; every other `Feature` variant is rejected by the core for every backend.

| Feature | ruby | bash | python | perl | go | java | codon |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Imported globals (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported memories (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported tables (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multiple tables | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Bulk table ops / passive element segments | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Floating-point (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Exception handling | ✅ | ❌ | ✅ | ✅ | ✅ | ✅ | ❌ |
| Tail calls | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |

## WASI preview 1

Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS stub. `—` marks the out-of-scope surface (sockets, `proc_raise`) no toolchain output exercises.

| Function | ruby | bash | python | perl | go | java | codon |
| --- | --- | --- | --- | --- | --- | --- | --- |
| args_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| args_sizes_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| environ_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| environ_sizes_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| clock_res_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| clock_time_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_advise | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_allocate | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_close | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_datasync | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_fdstat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_fdstat_set_flags | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_fdstat_set_rights | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_filestat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_filestat_set_size | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_filestat_set_times | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_pread | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_prestat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_prestat_dir_name | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_pwrite | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_read | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_readdir | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_renumber | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_seek | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_sync | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_tell | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| fd_write | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_create_directory | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_filestat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_filestat_set_times | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_link | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_open | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_readlink | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_remove_directory | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_rename | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_symlink | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| path_unlink_file | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| poll_oneoff | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ (ENOSYS) |
| proc_exit | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| proc_raise | — | — | — | — | — | — | — |
| random_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sched_yield | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sock_accept | — | — | — | — | — | — | — |
| sock_recv | — | — | — | — | — | — | — |
| sock_send | — | — | — | — | — | — | — |
| sock_shutdown | — | — | — | — | — | — | — |
