# See Rt::i64_rotl on the shift overflow behavior.
sub i64_rotr {
    my ($a, $b) = @_;
    my $r = $b & 63;
    return $r ? (($a >> $r) | ($a << (64 - $r))) & 0xFFFFFFFFFFFFFFFF : $a;
}
