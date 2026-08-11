// f32.min: a NaN operand yields wasm's canonical NaN (a legal min result and,
// being quiet, satisfies nan:arithmetic too, while Java's Math.min would pass a
// signaling operand through unquieted). min(-0, +0) is -0.
static float f32_min(float a, float b) {
    if (Float.isNaN(a) || Float.isNaN(b)) {
        return Float.intBitsToFloat(0x7fc00000);
    }
    if (a < b) {
        return a;
    }
    if (b < a) {
        return b;
    }
    if (a == 0.0f) {
        // Equal zeros: min is -0 if either operand is -0.
        return (Float.floatToRawIntBits(a) | Float.floatToRawIntBits(b)) < 0 ? -0.0f : 0.0f;
    }
    return a;
}
