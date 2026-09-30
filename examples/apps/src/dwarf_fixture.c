// dwarf_fixture.c: first-party DWARF fixture for the `--dwarf-line` source back-mapping test.
// `examples/apps/scripts/dwarf-fixture.sh` builds it with this command:
//   `wasi-sdk clang -g -O1 -o cache/dwarf-fixture.wasm`
//
// The two worker functions are exported and marked `noinline`.
// So `-O1` keeps them as distinct, addressable bodies with their own DWARF line-table rows.
// An inlined body or one removed as dead code would leave nothing for the test to check.
// The core test checks the DWARF address-base calibration by the known first line of a function.
// So the line numbers below matter to that test.
// Keep the body of `add_mul` starting where the test expects it.
// `main()` drives both functions with fixed inputs and prints one deterministic number.
// So the Go/Ruby e2e runs can compare their output with and without the flag.

#include <stdint.h>
#include <stdio.h>

// Integer arithmetic on two parameters.
// Its first statement (the multiply) is the row the core test calibrates against.
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
