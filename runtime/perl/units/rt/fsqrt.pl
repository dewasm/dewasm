# requires: rt/quiet_nan, rt/f64_from_bits
# Perl dies on sqrt of a negative; wasm wants NaN. sqrt(-0.0) is -0.0.
sub fsqrt {
    my $x = $_[0];
    return Rt::quiet_nan($x) if $x != $x;
    return Rt::f64_from_bits(0x7FF8000000000000) if $x < 0.0;
    return $x if $x == 0.0;
    return sqrt($x);
}
