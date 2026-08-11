// requires: rt/trap
// Signed i32 division with wasm's two trap conditions (Java's `/` traps on neither: it throws only on /0 and silently wraps INT_MIN/-1 to INT_MIN).
static int i32_div_s(int a, int b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    if (a == Integer.MIN_VALUE && b == -1) {
        trap("integer overflow");
    }
    return a / b;
}
