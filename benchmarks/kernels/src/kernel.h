/* Shared preamble for the C benchmark kernels.
 *
 * A kernel is a WASI command module invoked as `<module> <iterations>`. It does
 * <iterations> units of work, writes exactly one line -- the decimal result
 * followed by a newline -- to stdout, and exits 0. <iterations> = 0 does no
 * work but still prints, which is how the harness measures startup in
 * isolation.
 *
 * The suite compares wasmtime against pure-Ruby and pure-Python interpreters
 * that implement only part of WASI, so a kernel may import nothing beyond
 * args_sizes_get / args_get / fd_write / proc_exit. That rules out libc stdio,
 * whose buffered streams import fd_seek and fd_close as well -- imports are
 * resolved at instantiation, so merely linking them in would make the module
 * unloadable under wardite even if it never called them.
 *
 * Hence: the kernels are built with -nostartfiles and define their own _start,
 * bypassing crt1 and libc entirely. Everything the kernels need -- argv
 * parsing, decimal output -- is here, and it is deliberately small.
 *
 * See benchmarks/README.md for the full contract and the build flags.
 */

#ifndef DEWASM_BENCH_KERNEL_H
#define DEWASM_BENCH_KERNEL_H

typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long u64;

#define WASI_IMPORT(name) \
  __attribute__((import_module("wasi_snapshot_preview1"), import_name(name)))

WASI_IMPORT("args_sizes_get")
int wasi_args_sizes_get(u32 *argc, u32 *argv_buf_size);

WASI_IMPORT("args_get")
int wasi_args_get(char **argv, char *argv_buf);

WASI_IMPORT("fd_write")
int wasi_fd_write(int fd, const void *iovs, u32 iovs_len, u32 *nwritten);

WASI_IMPORT("proc_exit")
_Noreturn void wasi_proc_exit(int code);

struct kernel_iovec {
  const void *base;
  u32 len;
};

static void kernel_write(int fd, const char *buf, u32 len) {
  struct kernel_iovec iov = {buf, len};
  u32 nwritten;
  wasi_fd_write(fd, &iov, 1, &nwritten);
}

_Noreturn static void kernel_die(void) {
  static const char usage[] = "usage: <module> <iterations>\n";
  kernel_write(2, usage, sizeof(usage) - 1);
  wasi_proc_exit(2);
}

/* argv[1] as an unsigned decimal. The harness always passes exactly one
 * argument, so anything else is a caller bug, not an input to guess at. */
static u32 kernel_iterations(void) {
  static char argv_buf[256];
  static char *argv[8];
  u32 argc, buf_size;

  if (wasi_args_sizes_get(&argc, &buf_size) != 0) kernel_die();
  if (argc != 2) kernel_die();
  if (buf_size > sizeof(argv_buf)) kernel_die();
  if (wasi_args_get(argv, argv_buf) != 0) kernel_die();

  const char *p = argv[1];
  if (*p == '\0') kernel_die();
  u32 n = 0;
  for (; *p != '\0'; p++) {
    if ((u32)(*p - '0') > 9) kernel_die();
    n = n * 10 + (u32)(*p - '0');
  }
  return n;
}

/* Write `<v>\n` to stdout with v as an unsigned decimal. Digits come out least
 * significant first, so the scratch buffer is filled backwards. */
static void kernel_print(u64 v) {
  static char buf[24];
  char *end = buf + sizeof(buf);
  char *p = end;
  *--p = '\n';
  do {
    *--p = (char)('0' + (v % 10));
    v /= 10;
  } while (v != 0);
  kernel_write(1, p, (u32)(end - p));
}

/* Each kernel supplies both. kernel_setup runs before argv is even read, so any
 * fixed cost it has lands in the <iterations> = 0 baseline run the harness
 * subtracts, leaving per-iteration numbers pure; most kernels leave it empty.
 * It is an explicit hook rather than __attribute__((constructor)) because
 * -nostartfiles means nothing calls __wasm_call_ctors. */
static void kernel_setup(void);
static u64 kernel_run(u32 iterations);

__attribute__((export_name("_start"))) void _start(void) {
  kernel_setup();
  kernel_print(kernel_run(kernel_iterations()));
}

#endif /* DEWASM_BENCH_KERNEL_H */
