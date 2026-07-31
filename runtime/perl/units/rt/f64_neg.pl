# requires: rt/f64_bits, rt/f64_from_bits
sub f64_neg {
    return Rt::f64_from_bits(Rt::f64_bits($_[0]) ^ 0x8000000000000000);
}
