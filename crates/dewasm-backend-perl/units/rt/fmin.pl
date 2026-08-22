# requires: rt/f64_bits, rt/f64_from_bits
sub fmin {
    my ($a, $b) = @_;
    return Rt::f64_from_bits(0x7FF8000000000000) if $a != $a || $b != $b;
    return $a if $a < $b;
    return $b if $b < $a;
    if ($a == 0.0 && $b == 0.0) {
        return ((Rt::f64_bits($a) >> 63) | (Rt::f64_bits($b) >> 63)) ? -0.0 : 0.0;
    }
    return $a;
}
