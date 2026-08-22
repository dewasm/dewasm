// requires: rt/trap
static int i32_rem_u(int a, int b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    return Integer.remainderUnsigned(a, b);
}
