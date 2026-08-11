// dwarf_fixture.c: first-party DWARF fixture for the --dwarf-line source back-mapping test.
// Built by examples/apps/scripts/dwarf-fixture.sh with
// zig cc -target wasm32-wasi -g -O1 -o cache/dwarf-fixture.wasm
//
// The two worker functions are exported and marked noinline so that -O1 keeps them as distinct, addressable bodies with their own DWARF line-table rows
// (an inlined or dead-code-eliminated body would leave nothing to pin).
// The core test pins the DWARF address-base calibration by asserting a known function's first source line, so the line numbers below are load-bearing:
// keep add_mul's body starting where the test expects it. main() drives them with fixed inputs and prints one deterministic number, so the Go/Ruby e2e runs are byte-comparable with and without the flag.

#include <stdint.h>
#include <stdio.h>

// Integer arithmetic on two parameters.
// First statement (the multiply) is the row the core test calibrates against.
__attribute__((noinline, export_name("add_mul"))) int add_mul(int a, int b) {
    int product = a * b;
    int sum = a + b;
    return product + sum;
}

// A linear-memory access: sums the first n elements read through a pointer.
__attribute__((noinline, export_name("sum_prefix"))) int
sum_prefix(const int32_t *xs, int32_t n) {
    int32_t total = 0;
    for (int32_t i = 0; i < n; i++) {
        total += xs[i];
    }
    return total;
}

int main(void) {
    int32_t xs[4] = {2, 4, 6, 8};
    int r = add_mul(3, 5);
    int t = sum_prefix(xs, 4);
    printf("%d\n", r + t);
    return 0;
}
