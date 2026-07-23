# Backend Support Matrix

<!-- AUTO-GENERATED from the backend declarations; do not edit by hand.
     Regenerate: DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs -->

The spec harness only tolerates test skips attributable to a feature that is
not `Supported` here ([ADR-8](adr/8-latest-testsuite-support-matrix.md)); an
unattributable failure is treated as a bug. Flipping a feature to supported
turns its remaining skips into hard failures until the tests pass.

## Baseline

- **ruby**: Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, sign-extension, saturating float-to-int, multi-value, and the memory half of bulk memory (memory.copy/fill/init, passive data segments)

## Features

| Feature | ruby |
| --- | --- |
| Imported globals (wasm 1.0) | ❌ |
| Imported memories (wasm 1.0) | ❌ |
| Imported tables (wasm 1.0) | ❌ |
| Multiple tables | ❌ |
| Bulk table ops / passive element segments | ❌ |
| Reference types | ❌ |
| Typed function references | ❌ |
| Garbage collection | ❌ |
| Multiple memories | ❌ |
| Extended constant expressions | ❌ |
| SIMD | ❌ |
| Relaxed SIMD | ❌ |
| Threads and atomics | ❌ |
| Tail calls | ❌ |
| 64-bit memory | ❌ |
| Exception handling | ❌ |
| Wide arithmetic | ❌ |
| Custom page sizes | ❌ |

## WASI preview 1 (ruby)

Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS
stub ([ADR-7](adr/7-import-providers.md)).

| Function | ruby |
| --- | --- |
| args_get | ✅ |
| args_sizes_get | ✅ |
| environ_get | ✅ |
| environ_sizes_get | ✅ |
| clock_res_get | ✅ |
| clock_time_get | ✅ |
| fd_advise | ❌ (ENOSYS) |
| fd_allocate | ❌ (ENOSYS) |
| fd_close | ✅ |
| fd_datasync | ❌ (ENOSYS) |
| fd_fdstat_get | ✅ |
| fd_fdstat_set_flags | ❌ (ENOSYS) |
| fd_fdstat_set_rights | ❌ (ENOSYS) |
| fd_filestat_get | ❌ (ENOSYS) |
| fd_filestat_set_size | ❌ (ENOSYS) |
| fd_filestat_set_times | ❌ (ENOSYS) |
| fd_pread | ❌ (ENOSYS) |
| fd_prestat_get | ✅ |
| fd_prestat_dir_name | ❌ (ENOSYS) |
| fd_pwrite | ❌ (ENOSYS) |
| fd_read | ✅ |
| fd_readdir | ❌ (ENOSYS) |
| fd_renumber | ❌ (ENOSYS) |
| fd_seek | ✅ |
| fd_sync | ❌ (ENOSYS) |
| fd_tell | ✅ |
| fd_write | ✅ |
| path_create_directory | ❌ (ENOSYS) |
| path_filestat_get | ❌ (ENOSYS) |
| path_filestat_set_times | ❌ (ENOSYS) |
| path_link | ❌ (ENOSYS) |
| path_open | ❌ (ENOSYS) |
| path_readlink | ❌ (ENOSYS) |
| path_remove_directory | ❌ (ENOSYS) |
| path_rename | ❌ (ENOSYS) |
| path_symlink | ❌ (ENOSYS) |
| path_unlink_file | ❌ (ENOSYS) |
| poll_oneoff | ❌ (ENOSYS) |
| proc_exit | ✅ |
| proc_raise | ❌ (ENOSYS) |
| random_get | ✅ |
| sched_yield | ✅ |
| sock_accept | ❌ (ENOSYS) |
| sock_recv | ❌ (ENOSYS) |
| sock_send | ❌ (ENOSYS) |
| sock_shutdown | ❌ (ENOSYS) |
