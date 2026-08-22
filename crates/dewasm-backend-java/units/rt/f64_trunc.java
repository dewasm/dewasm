// requires: rt/f64_canon
// f64.trunc: round toward zero (see f32_trunc).
static double f64_trunc(double x) {
    return f64_canon(x < 0 ? Math.ceil(x) : Math.floor(x));
}
