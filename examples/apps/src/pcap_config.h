/*
 * pcap_config.h — first-party stand-in for libpcap's ./configure output
 * (ADR-9 permits committing first-party source). libpcap's build normally
 * generates config.h by probing the host; there is no wasm32-wasi host to
 * probe, so fetch.sh copies this file in as <config.h> on the include path
 * and compiles only the platform-independent BPF-compiler translation units
 * (no capture backend). It captures three things:
 *
 *   1. The handful of feature macros the filter compiler actually reads.
 *   2. Placeholders for the interface-lookup path that wasip1 cannot provide
 *      (socket()/SIOCGIF*): pcap_lookupnet() is compiled but never reached in
 *      a reactor build that only exports compile_filter, so wasm-ld garbage-
 *      collects it — these just let it *compile*.
 *   3. A baseline-wasm setjmp/longjmp stand-in. libpcap reports filter syntax
 *      errors by longjmp()ing back to a setjmp() at the top of pcap_compile();
 *      wasip1's <setjmp.h> refuses to compile without the wasm exception-
 *      handling proposal (out of dewasm's 0.1 scope, ADR-24). Since a *valid*
 *      filter — the only kind this demo compiles — never takes the error path,
 *      setjmp() collapses to 0 (always the success arm) and longjmp() to a
 *      trap. Compiling an invalid filter would trap instead of returning an
 *      error; that limitation is documented in pcap_binding.c's compile_filter.
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
/* wasip1 time_t is 64-bit; pcap-int.h hard-errors without this. */
#define SIZEOF_TIME_T 8

/* 2. Placeholders for the GC'd interface-lookup path (pcap_lookupnet). */
#ifndef SIOCGIFADDR
#define SIOCGIFADDR 0x8915
#endif
#ifndef SIOCGIFNETMASK
#define SIOCGIFNETMASK 0x891b
#endif
/* socket() is declared in <sys/socket.h> only for wasip2; declare it so the
 * (dead, GC'd) unix lookupnet body compiles under wasip1. */
extern int socket(int, int, int);

/* 3. Baseline-wasm setjmp/longjmp stand-in (see the header comment). Defining
 * the <setjmp.h> include guard makes the real header a no-op. */
#ifndef _SETJMP_H
#define _SETJMP_H
typedef long jmp_buf[1];
#define setjmp(buf) (0)
#define longjmp(buf, val) (__builtin_trap())
#endif

#endif /* DEWASM_PCAP_CONFIG_H */
