// requires: rt/trap
static long i64_rem_s(long a, long b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    if (a == Long.MIN_VALUE && b == -1) {
        return 0;
    }
    return a % b;
}
