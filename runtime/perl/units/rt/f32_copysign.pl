# requires: rt/f32_bits, rt/f32_from_bits
sub f32_copysign {
    return Rt::f32_from_bits((Rt::f32_bits($_[0]) & 0x7FFFFFFF) | (Rt::f32_bits($_[1]) & 0x80000000));
}
