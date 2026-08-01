# requires: rt/f32_bits, rt/f32_from_bits
sub f32_neg {
    return Rt::f32_from_bits(Rt::f32_bits($_[0]) ^ 0x80000000);
}
