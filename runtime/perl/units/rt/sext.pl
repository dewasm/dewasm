sub sext {
    my ($x, $bits, $mask) = @_;
    my $half = 1 << ($bits - 1);
    return ((($x & ((1 << $bits) - 1)) ^ $half) - $half) & $mask;
}
