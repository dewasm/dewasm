# requires: rt/f32_from_bits
sub f32_reinterpret_i32 {
    return Rt::f32_from_bits($_[0]);
}
