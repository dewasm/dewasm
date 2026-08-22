// requires: rt/trap
static long i64_rem_u(long a, long b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    return Long.remainderUnsigned(a, b);
}
