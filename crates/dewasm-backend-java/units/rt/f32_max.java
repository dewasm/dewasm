// f32.max: a NaN operand yields wasm's canonical NaN (see f32_min).
// max(-0, +0) is +0.
static float f32_max(float a, float b) {
    if (Float.isNaN(a) || Float.isNaN(b)) {
        return Float.intBitsToFloat(0x7fc00000);
    }
    if (a > b) {
        return a;
    }
    if (b > a) {
        return b;
    }
    if (a == 0.0f) {
        // Equal zeros: max is -0 only if both operands are -0.
        return (Float.floatToRawIntBits(a) & Float.floatToRawIntBits(b)) < 0 ? -0.0f : 0.0f;
    }
    return a;
}
