# UV << wraps mod 2^64 (measured), so the overflowing half just
# needs the final mask.
sub i64_rotl {
    my ($a, $b) = @_;
    my $r = $b & 63;
    return $r ? (($a << $r) | ($a >> (64 - $r))) & 0xFFFFFFFFFFFFFFFF : $a;
}
