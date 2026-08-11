# requires: rt/f32
# Convert a signed integer to f32 with correct rounding. Values beyond 2^53
# are pre-rounded to odd so the int->double->single chain cannot
# double-round.
sub cvt_f32_i {
    my $v = $_[0];
    my $a = $v < 0 ? -$v : $v;
    return Rt::f32(0.0 + $v) if $a < 9007199254740992;
    my $sh = length(sprintf('%b', $a)) - 53;
    my $hi = $a >> $sh;
    $hi |= 1 if ($hi << $sh) != $a;
    $hi = -$hi if $v < 0;
    return Rt::f32((0.0 + $hi) * (2.0 ** $sh));
}
