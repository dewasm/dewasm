# Round a double to single precision. pack 'f' clamps out-of-float-range values straight to infinity instead of IEEE-rounding them (measured), so finite inputs below the rounding boundary (2^128 - 2^103) must map back to the largest finite f32 by hand.
sub f32 {
    my $x = $_[0];
    my $r = unpack('f<', pack('f<', $x));
    if (($r == 9**9**9 || $r == -9**9**9)
        && $x > -3.4028235677973366e38 && $x < 3.4028235677973366e38) {
        return $x > 0 ? 3.4028234663852886e38 : -3.4028234663852886e38;
    }
    return $r;
}
