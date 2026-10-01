/*
 * pcap_config.h: first-party stand-in for the `./configure` output of `libpcap`.
 * First-party source is fine to commit.
 * The build of `libpcap` normally generates `config.h` by probing the host.
 * There is no `wasm32-wasi` host to probe.
 * So `scripts/libpcap.sh` copies this file in as `<config.h>` on the include path.
 * It compiles only the platform-independent BPF-compiler translation units (no capture backend).
 * This file holds three things:
 *
 *   1. The few feature macros the filter compiler actually reads.
 *   2. Placeholders for the interface-lookup path that `wasip1` cannot provide.
 *      That path needs `socket()`/`SIOCGIF*`.
 *      `pcap_lookupnet()` is compiled but never reached in a reactor build.
 *      That build only exports `compile_filter`, so `wasm-ld` removes it as unused code.
 *      These placeholders just let it *compile*.
 *   3. A baseline-wasm `setjmp`/`longjmp` stand-in.
 *      `libpcap` reports filter syntax errors by a `longjmp()` back to a `setjmp()`.
 *      That `setjmp()` sits at the top of `pcap_compile()`.
 *      The `<setjmp.h>` of `wasip1` fails to compile without the wasm exception-handling proposal.
 *      That proposal is out of the 0.1 scope of dewasm.
 *      A *valid* filter (the only kind this example compiles) never takes the error path.
 *      So `setjmp()` collapses to 0 (always the success arm), and `longjmp()` to a trap.
 *      Compiling an invalid filter would trap instead of returning an error.
 *      The `compile_filter` of `pcap_binding.c` documents that limitation.
 */
#ifndef DEWASM_PCAP_CONFIG_H
#define DEWASM_PCAP_CONFIG_H

/* 1. Feature macros the BPF compiler reads. */
#define PACKAGE_NAME "libpcap"
#define PACKAGE_STRING "libpcap 1.10.6"
#define PACKAGE_VERSION "1.10.6"
#define HAVE_SNPRINTF 1
#define HAVE_VSNPRINTF 1
#define HAVE_STRERROR 1
#define HAVE_STRTOK_R 1
#define HAVE_STRUCT_SOCKADDR_STORAGE 1
/* The `time_t` of `wasip1` is 64-bit; `pcap-int.h` fails with an error without this. */
#define SIZEOF_TIME_T 8

/* 2. Placeholders for the interface-lookup path removed as unused code (`pcap_lookupnet`). */
#ifndef SIOCGIFADDR
#define SIOCGIFADDR 0x8915
#endif
#ifndef SIOCGIFNETMASK
#define SIOCGIFNETMASK 0x891b
#endif
/*
 * `socket()` is declared in `<sys/socket.h>` only for `wasip2`.
 * Declare it so the Unix `lookupnet` body compiles under `wasip1`.
 * That body is dead code, removed as unused.
 */
extern int socket(int, int, int);

/*
 * 3. Baseline-wasm `setjmp`/`longjmp` stand-in (see the header comment).
 * Defining the `<setjmp.h>` include guard makes the real header a no-op.
 */
#ifndef _SETJMP_H
#define _SETJMP_H
typedef long jmp_buf[1];
#define setjmp(buf) (0)
#define longjmp(buf, val) (__builtin_trap())
#endif

#endif /* DEWASM_PCAP_CONFIG_H */
