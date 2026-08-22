# See Rt::i64_add: wrap mod 2^64 in native integer arithmetic.
sub i64_sub {
    return (do { use integer; $_[0] - $_[1] }) & 0xFFFFFFFFFFFFFFFF;
}
