# i64 sums can overflow the UV range, where plain Perl arithmetic goes through NVs.
# That loses precision, so wrap modulo 2^64 in native integer arithmetic and mask.
sub i64_add {
    return (do { use integer; $_[0] + $_[1] }) & 0xFFFFFFFFFFFFFFFF;
}
