// requires: rt/trap
static long i64_div_u(long a, long b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    return Long.divideUnsigned(a, b);
}
