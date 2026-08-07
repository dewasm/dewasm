# i64 sums can overflow the UV range, where plain perl arithmetic goes
# through NVs and loses precision; wrap mod 2^64 in native integer
# arithmetic and mask.
sub i64_add {
    return (do { use integer; $_[0] + $_[1] }) & 0xFFFFFFFFFFFFFFFF;
}
