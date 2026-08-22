# requires: rt/f32_bits
sub i32_reinterpret_f32 {
    return Rt::f32_bits($_[0]);
}
