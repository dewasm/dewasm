// requires: rt/trap
// Signed i32 remainder: traps only on /0; INT_MIN % -1 is 0 (no overflow trap), which Java's `%` already yields.
static int i32_rem_s(int a, int b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    if (a == Integer.MIN_VALUE && b == -1) {
        return 0;
    }
    return a % b;
}
