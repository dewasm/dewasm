// requires: rt/trap
static long i64_div_s(long a, long b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    if (a == Long.MIN_VALUE && b == -1) {
        trap("integer overflow");
    }
    return a / b;
}
