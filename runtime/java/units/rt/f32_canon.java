// Canonicalize a NaN produced by an arithmetic float op to wasm's canonical
// NaN. Java's Math.* library ops (ceil/floor/trunc/nearest/sqrt) may pass a
// signaling NaN operand through unquieted, but wasm requires an arithmetic
// (quiet) NaN result; the canonical NaN is a legal arithmetic result and also
// satisfies nan:canonical. Non-NaN values pass through untouched.
static float f32_canon(float x) {
    return Float.isNaN(x) ? Float.intBitsToFloat(0x7fc00000) : x;
}
