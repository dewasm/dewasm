# The signed view of a masked-unsigned i64: `use integer` arithmetic reads the raw 64-bit slot, so `+ 0` reinterprets the UV bit pattern as an IV
# (measured).
sub s64 {
    return $_[0] < 0x8000000000000000 ? $_[0] : do { use integer; $_[0] + 0 };
}
