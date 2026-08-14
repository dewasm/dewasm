# Backend Support Matrix

<!-- AUTO-GENERATED from the backend declarations; do not edit by hand. Regenerate: cargo xtask update-support-docs -->

The spec harness only tolerates test skips attributable to a feature that is not `Supported` here; an unattributable failure is treated as a bug. Flipping a feature to supported turns its remaining skips into hard failures until the tests pass.

## Features

The features a backend can meaningfully differ on; every other `Feature` variant is rejected by the core for every backend.

| Feature | ruby | bash | python | perl | go | java |
| --- | --- | --- | --- | --- | --- | --- |
| Imported globals (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported memories (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Imported tables (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multiple tables | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Bulk table ops / passive element segments | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Floating-point (wasm 1.0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Exception handling | ✅ | ❌ | ✅ | ❌ | ✅ | ✅ |

## WASI preview 1

Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS stub. `—` marks the out-of-scope surface (sockets, `proc_raise`) no toolchain output exercises.

| Function | ruby | bash | python | perl | go | java |
| --- | --- | --- | --- | --- | --- | --- |
| args_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| args_sizes_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| environ_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| environ_sizes_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| clock_res_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| clock_time_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_advise | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_allocate | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_close | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_datasync | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_fdstat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_fdstat_set_flags | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_fdstat_set_rights | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_filestat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_filestat_set_size | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_filestat_set_times | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ | ✅ |
| fd_pread | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_prestat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_prestat_dir_name | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_pwrite | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_read | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_readdir | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_renumber | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_seek | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_sync | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_tell | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| fd_write | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_create_directory | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_filestat_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_filestat_set_times | ✅ | ❌ (ENOSYS) | ✅ | ✅ | ✅ | ✅ |
| path_link | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_open | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_readlink | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_remove_directory | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_rename | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_symlink | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| path_unlink_file | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| poll_oneoff | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| proc_exit | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| proc_raise | — | — | — | — | — | — |
| random_get | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sched_yield | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sock_accept | — | — | — | — | — | — |
| sock_recv | — | — | — | — | — | — |
| sock_send | — | — | — | — | — | — |
| sock_shutdown | — | — | — | — | — | — |
