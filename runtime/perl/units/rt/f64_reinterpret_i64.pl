# requires: rt/f64_from_bits
sub f64_reinterpret_i64 {
    return Rt::f64_from_bits($_[0]);
}
