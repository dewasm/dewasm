/*
 * pcap_binding.c: our own committed C source (first-party source is
 * fine to commit; only third-party *artifacts* stay out of the tree).
 *
 * A reactor library exporting a single BPF-filter-compilation entry point on
 * top of libpcap's platform-independent compiler (gencode.c/optimize.c). No
 * capture backend is built (see src/pcap_config.h); only pcap_compile_nopcap()
 * (which turns a textual filter like "tcp port 80" into a BPF program) is
 * reachable. Built into cache/libpcap.wasm by examples/apps/scripts/libpcap.sh from the
 * pinned upstream release, with the same wasi-sdk reactor flags as the
 * sqlite3 apps. The headers and resolver stand-ins the wasi build needs
 * beyond wasi-libc live in src/pcap_wasi.
 */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <pcap/pcap.h>

/*
 * Compile the textual filter `expr` for datalink type `linktype` (e.g.
 * DLT_EN10MB == 1) with capture length `snaplen`, and serialize the resulting
 * BPF program into a freshly malloc'd buffer in guest memory. Layout (all
 * little-endian, tightly packed, no padding):
 *
 *   [u32 bf_len]:                                number of BPF instructions
 *   bf_len x { u16 code; u8 jt; u8 jf; u32 k }:  8 bytes each
 *
 * Returns the guest pointer to that buffer, or 0 on any error (compile
 * failure or out-of-memory). The caller reads bf_len, then that many 8-byte
 * instructions, and frees the buffer with free(). NB: an *invalid* filter
 * expression traps rather than returning 0: libpcap reports filter syntax
 * errors via longjmp, and the baseline-wasm setjmp/longjmp stand-in in
 * src/pcap_config.h turns that unwind into a trap. Valid filters, the only
 * ones this demo drives, compile and serialize normally.
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
