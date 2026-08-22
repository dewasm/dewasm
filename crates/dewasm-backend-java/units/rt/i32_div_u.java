// requires: rt/trap
static int i32_div_u(int a, int b) {
    if (b == 0) {
        trap("integer divide by zero");
    }
    return Integer.divideUnsigned(a, b);
}
