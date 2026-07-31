# requires: rt/f64_bits
# pack's float<->double conversion canonicalizes NaNs, losing the payload
# (measured, ADR-55). Take a software path for NaNs (ADR-2).
sub f32_bits {
    my $x = $_[0];
    if ($x != $x) {
        my $b = Rt::f64_bits($x);
        my $payload = ($b >> 29) & 0x7FFFFF;
        $payload = 0x400000 if $payload == 0;
        return ((($b >> 63) & 1) << 31) | 0x7F800000 | $payload;
    }
    return unpack('L<', pack('f<', $x));
}
