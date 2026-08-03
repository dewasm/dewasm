/* wordcount -- memory- and branch-heavy scanning over a generated buffer.
 *
 * One iteration consumes one byte of text: a load, a handful of unpredictable
 * branches, and a scattered histogram update. Where sha256 is a straight-line
 * arithmetic pipeline, this microbenchmark is what a backend's branch and array
 * lowering actually costs -- the buffer is pseudo-random, so no branch here
 * predicts and no histogram slot stays hot.
 *
 * The text buffer is generated once at startup, before the iteration count is
 * even consulted, so its cost lands in the <iterations> = 0 baseline run the
 * harness subtracts. Per-iteration numbers stay pure scanning as a result.
 * <iterations> may exceed the buffer size; the scan wraps.
 *
 * The result folds words, lines and the longest word into one u64.
 */

#include "bench.h"

#define TEXT_SIZE 8192

/* 32 usable entries so the generator can mask instead of dividing: 26 letters,
 * four spaces and two newlines give roughly six-letter words and forty-word
 * lines. (The trailing NUL is entry 32 and is never indexed.) */
static const char ALPHABET[] = "abcdefghijklmnopqrstuvwxyz    \n\n";

static char text[TEXT_SIZE];

/* Runs before _start reads argv, so the generation cost is in the baseline. */
static void bench_setup(void) {
  u32 h = 0x9e3779b9u;
  for (u32 i = 0; i < TEXT_SIZE; i++) {
    h = h * 1664525u + 1013904223u;
    text[i] = ALPHABET[(h >> 24) & 31u];
  }
}

/* Static, not a zero-initialized local: -nostdlib leaves no memset to call, and
 * clang lowers a local `= {0}` of this size to one. Statics live in bss, which
 * the engine zeroes at instantiation for free. */
static u32 histogram[26];

static u64 bench_run(u32 iterations) {
  u64 words = 0, lines = 0;
  u32 max_len = 0, cur_len = 0;
  u32 pos = 0;

  for (u32 i = 0; i < iterations; i++) {
    char c = text[pos];
    if (++pos == TEXT_SIZE) pos = 0;

    if (c == ' ' || c == '\n') {
      if (cur_len != 0) {
        words++;
        if (cur_len > max_len) max_len = cur_len;
        cur_len = 0;
      }
      if (c == '\n') lines++;
    } else {
      cur_len++;
      histogram[(u32)(c - 'a')]++;
    }
  }

  /* A trailing partial word counts, so the result does not depend on where the
   * scan happened to stop mid-token. */
  if (cur_len != 0) {
    words++;
    if (cur_len > max_len) max_len = cur_len;
  }

  u64 fold = words * 1000003u + lines * 31u + max_len;
  for (u32 i = 0; i < 26; i++) fold += (u64)histogram[i] * (u64)(i + 1);
  return fold;
}
