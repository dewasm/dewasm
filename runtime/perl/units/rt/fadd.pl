# requires: rt/f64_bits
# Perl's arithmetic operators take an integer fast path for integral operands: it yields integer-exact results beyond double precision and turns -0.0 operands into +0 (measured).
# The pack 'd' round-trip restores IEEE rounding, and exact-zero results get their IEEE sign back by hand: a zero sum is negative only when both operands are -0.0.
sub fadd {
    my ($a, $b) = @_;
    my $r = unpack('d<', pack('d<', $a + $b));
    return -0.0 if $r == 0.0 && (Rt::f64_bits($a) >> 63) && (Rt::f64_bits($b) >> 63);
    return $r;
}
