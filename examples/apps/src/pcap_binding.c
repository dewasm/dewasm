/*
 * pcap_binding.c: our own committed C source.
 * First-party source is fine to commit; only third-party *artifacts* stay out of the tree.
 *
 * A reactor library exporting a single BPF-filter-compilation entry point.
 * It sits on top of the platform-independent compiler of `libpcap` (`gencode.c`/`optimize.c`).
 * No capture backend is built (see `src/pcap_config.h`).
 * Only `pcap_compile_nopcap()` is reachable.
 * It turns a filter written as text, like "tcp port 80", into a BPF program.
 * `examples/apps/scripts/libpcap.sh` builds this into `cache/libpcap.wasm`.
 * It builds from the upstream release at a fixed version.
 * It uses the same `wasi-sdk` reactor flags as the `sqlite3` apps.
 * The WASI build needs headers and resolver stand-ins beyond `wasi-libc`.
 * They live in `src/pcap_wasi`.
 */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <pcap/pcap.h>

/*
 * Compile the filter text `expr` for the data link type `linktype`, with capture length `snaplen`.
 * An example of `linktype` is `DLT_EN10MB == 1`.
 * Serialize the resulting BPF program into a new `malloc`'d buffer in guest memory.
 * Layout (all little-endian, tightly packed, no padding):
 *
 *   `[u32 bf_len]`:                                number of BPF instructions
 *   `bf_len x { u16 code; u8 jt; u8 jf; u32 k }`:  8 bytes each
 *
 * Returns the guest pointer to that buffer, or 0 on any error (compile failure or out-of-memory).
 * The caller reads `bf_len`, then that many 8-byte instructions.
 * It then frees the buffer with `free()`.
 * Note: an *invalid* filter expression traps rather than returning 0.
 * `libpcap` reports filter syntax errors with `longjmp`.
 * The baseline-wasm `setjmp`/`longjmp` stand-in turns that unwind into a trap.
 * That stand-in is in `src/pcap_config.h`.
 * Valid filters, the only ones this example drives, compile and serialize normally.
 */
uint8_t *compile_filter(const char *expr, int linktype, int snaplen) {
  struct bpf_program prog;
  if (pcap_compile_nopcap(snaplen, linktype, &prog, expr, 1,
                          PCAP_NETMASK_UNKNOWN) != 0) {
    return NULL;
  }

  uint32_t n = prog.bf_len;
  uint8_t *out = (uint8_t *)malloc(4 + (size_t)n * 8);
  if (out == NULL) {
    pcap_freecode(&prog);
    return NULL;
  }

  memcpy(out, &n, 4);
  for (uint32_t i = 0; i < n; i++) {
    const struct bpf_insn *in = &prog.bf_insns[i];
    uint8_t *p = out + 4 + (size_t)i * 8;
    uint16_t code = in->code;
    uint32_t k = in->k;
    memcpy(p + 0, &code, 2);
    p[2] = in->jt;
    p[3] = in->jf;
    memcpy(p + 4, &k, 4);
  }

  pcap_freecode(&prog);
  return out;
}
