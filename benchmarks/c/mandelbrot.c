/* mandelbrot -- f64-heavy, with an integer result.
 *
 * One iteration samples one point of the complex plane and runs the escape
 * loop on it, up to MAX_ESCAPE steps. The inner loop is nothing but f64 mul,
 * add and compare, all of which are correctly rounded on every runtime in the
 * suite, so the escape counts are identical everywhere.
 *
 * The result is the total escape-iteration count -- an integer. Printing a
 * double instead would compare Ruby/Python/Perl/Go/Java float formatting rather
 * than the arithmetic, and the harness compares stdout byte for byte.
 *
 * Sample points come from a multiplicative hash of the index rather than a
 * raster scan, so the iteration count is a free parameter instead of being tied
 * to an image size, and successive points land in unrelated parts of the set --
 * the escape loop's trip count varies wildly, which is the point.
 */

#include "bench.h"

#define MAX_ESCAPE 100

static void bench_setup(void) {}

static u64 bench_run(u32 iterations) {
  u64 total = 0;

  for (u32 k = 0; k < iterations; k++) {
    u32 h = k * 2654435761u + 12345u;
    /* Both halves of the hash map exactly onto a dyadic grid: the integer is
     * exact as a double and the scale is a power of two, so the sample points
     * are bit-identical on every runtime. */
    double cr = -2.0 + 2.5 * ((double)(h & 0xffffu) * (1.0 / 65536.0));
    double ci = -1.25 + 2.5 * ((double)((h >> 16) & 0xffffu) * (1.0 / 65536.0));

    double zr = 0.0, zi = 0.0;
    u32 n = 0;
    while (n < MAX_ESCAPE) {
      double zr2 = zr * zr;
      double zi2 = zi * zi;
      if (zr2 + zi2 > 4.0) break;
      zi = 2.0 * zr * zi + ci;
      zr = zr2 - zi2 + cr;
      n++;
    }
    total += n;
  }

  return total;
}
