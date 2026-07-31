sub i32_rotl {
    my ($a, $b) = @_;
    my $r = $b & 31;
    return $r ? (($a << $r) | ($a >> (32 - $r))) & 0xFFFFFFFF : $a;
}
