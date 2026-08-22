# requires: rt/f64_bits, rt/f64_from_bits
# Quiet a NaN (set the quiet bit), preserving sign and payload.
sub quiet_nan {
    return Rt::f64_from_bits(Rt::f64_bits($_[0]) | 0x0008000000000000);
}
