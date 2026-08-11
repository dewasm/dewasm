# requires: rt/quiet_nan, rt/f64_bits, rt/f64_from_bits
# Perl dies on float division by zero, so IEEE division goes through a
# helper (wasm requires inf/nan there, never an error). A NaN dividend must
# come out quiet (arithmetic-NaN result), and the zero-divisor shortcut
# skips the hardware division that would quiet it. The nonzero path
# needs Rt::fadd's integer-fast-path countermeasures too (measured): the
# pack 'd' round-trip for exact even divisions beyond 2^53, and the sign
# XOR for zero (including underflowed) quotients.
sub fdiv {
    my ($a, $b) = @_;
    if ($b != 0.0) {
        my $r = unpack('d<', pack('d<', $a / $b));
        return -0.0 if $r == 0.0 && ((Rt::f64_bits($a) >> 63) ^ (Rt::f64_bits($b) >> 63));
        return $r;
    }
    return Rt::quiet_nan($a) if $a != $a;
    return Rt::f64_from_bits(0x7FF8000000000000) if $a == 0.0;
    my $neg = ((Rt::f64_bits($a) >> 63) ^ (Rt::f64_bits($b) >> 63)) & 1;
    return $neg ? -9**9**9 : 9**9**9;
}
