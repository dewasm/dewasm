# requires: rt/f64_from_bits
# See Rt::f32_bits: NaNs travel through a software widening so sign and payload survive.
sub f32_from_bits {
    my $b = $_[0];
    if (($b & 0x7F800000) == 0x7F800000 && ($b & 0x7FFFFF) != 0) {
        return Rt::f64_from_bits((($b >> 31) << 63) | 0x7FF0000000000000 | (($b & 0x7FFFFF) << 29));
    }
    return unpack('f<', pack('L<', $b));
}
