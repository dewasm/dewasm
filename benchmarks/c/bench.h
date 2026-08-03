/* Shared preamble for the C microbenchmarks.
 *
 * A microbenchmark is a WASI command module invoked as `<module> <iterations>`. It does
 * <iterations> units of work, writes exactly one line -- the decimal result
 * followed by a newline -- to stdout, and exits 0. <iterations> = 0 does no
 * work but still prints, which is how the harness measures startup in
 * isolation.
 *
 * The suite compares wasmtime against pure-Ruby and pure-Python interpreters
 * that implement only part of WASI, so a microbenchmark may import nothing beyond
 * args_sizes_get / args_get / fd_write / proc_exit. That rules out libc stdio,
 * whose buffered streams import fd_seek and fd_close as well -- imports are
 * resolved at instantiation, so merely linking them in would make the module
 * unloadable under wardite even if it never called them.
 *
 * Hence: the microbenchmarks are built with -nostartfiles and define their own _start,
 * bypassing crt1 and libc entirely. Everything they need -- argv
 * parsing, decimal output -- is here, and it is deliberately small.
 *
 * The measurement design and the runner-intersection constraint are ADR-57; the flags are documented in c/build.sh.
 */

#ifndef DEWASM_BENCH_H
#define DEWASM_BENCH_H

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

struct bench_iovec {
  const void *base;
  u32 len;
};

static void bench_write(int fd, const char *buf, u32 len) {
  struct bench_iovec iov = {buf, len};
  u32 nwritten;
  wasi_fd_write(fd, &iov, 1, &nwritten);
}

_Noreturn static void bench_die(void) {
  static const char usage[] = "usage: <module> <iterations>\n";
  bench_write(2, usage, sizeof(usage) - 1);
  wasi_proc_exit(2);
}

/* argv[1] as an unsigned decimal. The harness always passes exactly one
 * argument, so anything else is a caller bug, not an input to guess at. */
static u32 bench_iterations(void) {
  static char argv_buf[256];
  static char *argv[8];
  u32 argc, buf_size;

  if (wasi_args_sizes_get(&argc, &buf_size) != 0) bench_die();
  if (argc != 2) bench_die();
  if (buf_size > sizeof(argv_buf)) bench_die();
  if (wasi_args_get(argv, argv_buf) != 0) bench_die();

  const char *p = argv[1];
  if (*p == '\0') bench_die();
  u32 n = 0;
  for (; *p != '\0'; p++) {
    if ((u32)(*p - '0') > 9) bench_die();
    n = n * 10 + (u32)(*p - '0');
  }
  return n;
}

/* Write `<v>\n` to stdout with v as an unsigned decimal. Digits come out least
 * significant first, so the scratch buffer is filled backwards. */
static void bench_print(u64 v) {
  static char buf[24];
  char *end = buf + sizeof(buf);
  char *p = end;
  *--p = '\n';
  do {
    *--p = (char)('0' + (v % 10));
    v /= 10;
  } while (v != 0);
  bench_write(1, p, (u32)(end - p));
}

/* Each microbenchmark supplies both. bench_setup runs before argv is even read, so any
 * fixed cost it has lands in the <iterations> = 0 baseline run the harness
 * subtracts, leaving per-iteration numbers pure; most leave it empty.
 * It is an explicit hook rather than __attribute__((constructor)) because
 * -nostartfiles means nothing calls __wasm_call_ctors. */
static void bench_setup(void);
static u64 bench_run(u32 iterations);

__attribute__((export_name("_start"))) void _start(void) {
  bench_setup();
  bench_print(bench_run(bench_iterations()));
}

#endif /* DEWASM_BENCH_H */
