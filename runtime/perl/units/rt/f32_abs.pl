# requires: rt/f32_bits, rt/f32_from_bits
sub f32_abs {
    return Rt::f32_from_bits(Rt::f32_bits($_[0]) & 0x7FFFFFFF);
}
