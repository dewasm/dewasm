# requires: rt/f64_bits, rt/f64_from_bits
sub f64_abs {
    return Rt::f64_from_bits(Rt::f64_bits($_[0]) & 0x7FFFFFFFFFFFFFFF);
}
