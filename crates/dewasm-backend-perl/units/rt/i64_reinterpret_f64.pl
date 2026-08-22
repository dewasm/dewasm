# requires: rt/f64_bits
sub i64_reinterpret_f64 {
    return Rt::f64_bits($_[0]);
}
