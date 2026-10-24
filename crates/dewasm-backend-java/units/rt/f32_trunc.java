// requires: rt/f32_canon
// f32.trunc: round toward zero.
// A helper, not an inline `a<0 ? ceil : floor`: it reads the operand once and canonicalizes a NaN.
static float f32_trunc(float x) {
    return f32_canon((float) (x < 0 ? Math.ceil(x) : Math.floor(x)));
}
