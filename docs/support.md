# Backend Support Matrix

<!-- AUTO-GENERATED from the backend declarations; do not edit by hand.
     Regenerate: DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs -->

The spec harness only tolerates test skips attributable to a feature that is
not `Supported` here ([ADR-8](adr/8-latest-testsuite-support-matrix.md)); an
unattributable failure is treated as a bug. Flipping a feature to supported
turns its remaining skips into hard failures until the tests pass.

## Baseline

- **ruby**: Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, sign-extension, saturating float-to-int, multi-value, and the memory half of bulk memory (memory.copy/fill/init, passive data segments)
- **bash**: Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, sign-extension, saturating float-to-int, multi-value, and the memory half of bulk memory; f32/f64 run on the pure-Bash softfloat (ADR-5/ADR-13); requires bash >= 5

## Features

| Feature | ruby | bash |
| --- | --- | --- |
| Imported globals (wasm 1.0) | ✅ | ❌ |
| Imported memories (wasm 1.0) | ✅ | ❌ |
| Imported tables (wasm 1.0) | ✅ | ❌ |
| Multiple tables | ✅ | ❌ |
| Bulk table ops / passive element segments | ✅ | ❌ |
| Floating-point (wasm 1.0) | ✅ | ✅ |
| Reference types | ✅ | ❌ |
| Typed function references | ❌ | ❌ |
| Garbage collection | ❌ | ❌ |
| Multiple memories | ❌ | ❌ |
| Extended constant expressions | ❌ | ❌ |
| SIMD | ❌ | ❌ |
| Relaxed SIMD | ❌ | ❌ |
| Threads and atomics | ❌ | ❌ |
| Tail calls | ✅ | ❌ |
| 64-bit memory | ❌ | ❌ |
| Exception handling | ✅ | ❌ |
| Wide arithmetic | ❌ | ❌ |
| Custom page sizes | ❌ | ❌ |
| Component model / WASI preview 2 | 🟡 single-world command components; .wast component directives not executed | ❌ |

## WASI preview 1

Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS
stub ([ADR-7](adr/7-import-providers.md), bash conventions in
[ADR-12](adr/12-bash-wasi.md)).

| Function | ruby | bash |
| --- | --- | --- |
| args_get | ✅ | ✅ |
| args_sizes_get | ✅ | ✅ |
| environ_get | ✅ | ✅ |
| environ_sizes_get | ✅ | ✅ |
| clock_res_get | ✅ | ✅ |
| clock_time_get | ✅ | ✅ |
| fd_advise | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_allocate | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_close | ✅ | ✅ |
| fd_datasync | ✅ | ❌ (ENOSYS) |
| fd_fdstat_get | ✅ | ✅ |
| fd_fdstat_set_flags | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_fdstat_set_rights | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_filestat_get | ✅ | ❌ (ENOSYS) |
| fd_filestat_set_size | ✅ | ❌ (ENOSYS) |
| fd_filestat_set_times | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_pread | ✅ | ❌ (ENOSYS) |
| fd_prestat_get | ✅ | ✅ |
| fd_prestat_dir_name | ✅ | ❌ (ENOSYS) |
| fd_pwrite | ✅ | ❌ (ENOSYS) |
| fd_read | ✅ | ✅ |
| fd_readdir | ✅ | ❌ (ENOSYS) |
| fd_renumber | ❌ (ENOSYS) | ❌ (ENOSYS) |
| fd_seek | ✅ | ✅ |
| fd_sync | ✅ | ❌ (ENOSYS) |
| fd_tell | ✅ | ✅ |
| fd_write | ✅ | ✅ |
| path_create_directory | ✅ | ❌ (ENOSYS) |
| path_filestat_get | ✅ | ❌ (ENOSYS) |
| path_filestat_set_times | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_link | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_open | ✅ | ❌ (ENOSYS) |
| path_readlink | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_remove_directory | ✅ | ❌ (ENOSYS) |
| path_rename | ✅ | ❌ (ENOSYS) |
| path_symlink | ❌ (ENOSYS) | ❌ (ENOSYS) |
| path_unlink_file | ✅ | ❌ (ENOSYS) |
| poll_oneoff | ❌ (ENOSYS) | ❌ (ENOSYS) |
| proc_exit | ✅ | ✅ |
| proc_raise | ❌ (ENOSYS) | ❌ (ENOSYS) |
| random_get | ✅ | ✅ |
| sched_yield | ✅ | ✅ |
| sock_accept | ❌ (ENOSYS) | ❌ (ENOSYS) |
| sock_recv | ❌ (ENOSYS) | ❌ (ENOSYS) |
| sock_send | ❌ (ENOSYS) | ❌ (ENOSYS) |
| sock_shutdown | ❌ (ENOSYS) | ❌ (ENOSYS) |

## WASI preview 2 (ruby, components only)

The `Rt::WASIP2` host functions the runtime units implement
([ADR-21](adr/21-ruby-wasi-preview2.md)); a component importing an
unimplemented `wasi:*` function still links but traps if it calls it.

- `cli_environment_get_arguments`
- `cli_environment_get_environment`
- `cli_exit_exit`
- `cli_stderr_get_stderr`
- `cli_stdin_get_stdin`
- `cli_stdout_get_stdout`
- `cli_terminal_stderr_get_terminal_stderr`
- `cli_terminal_stdin_get_terminal_stdin`
- `cli_terminal_stdout_get_terminal_stdout`
- `clocks_monotonic_clock_now`
- `clocks_monotonic_clock_resolution`
- `clocks_monotonic_clock_subscribe_duration`
- `clocks_monotonic_clock_subscribe_instant`
- `clocks_wall_clock_now`
- `filesystem_preopens_get_directories`
- `filesystem_types_method_descriptor_append_via_stream`
- `filesystem_types_method_descriptor_get_flags`
- `filesystem_types_method_descriptor_get_type`
- `filesystem_types_method_descriptor_metadata_hash`
- `filesystem_types_method_descriptor_open_at`
- `filesystem_types_method_descriptor_read_via_stream`
- `filesystem_types_method_descriptor_stat`
- `filesystem_types_method_descriptor_write_via_stream`
- `io_error_method_error_to_debug_string`
- `io_poll_method_pollable_block`
- `io_poll_poll`
- `io_streams_method_input_stream_blocking_read`
- `io_streams_method_input_stream_read`
- `io_streams_method_input_stream_subscribe`
- `io_streams_method_output_stream_blocking_flush`
- `io_streams_method_output_stream_blocking_write_and_flush`
- `io_streams_method_output_stream_check_write`
- `io_streams_method_output_stream_flush`
- `io_streams_method_output_stream_subscribe`
- `io_streams_method_output_stream_write`
- `random_get_random_bytes`
