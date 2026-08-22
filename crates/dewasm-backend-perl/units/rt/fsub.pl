# requires: rt/f64_bits
# See Rt::fadd; a zero difference is negative only for (-0.0) - (+0.0).
sub fsub {
    my ($a, $b) = @_;
    my $r = unpack('d<', pack('d<', $a - $b));
    return -0.0 if $r == 0.0 && (Rt::f64_bits($a) >> 63) && !(Rt::f64_bits($b) >> 63);
    return $r;
}
