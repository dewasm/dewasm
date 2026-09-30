# An i32 product can exceed 2^53, where plain Perl arithmetic goes through NVs and loses precision.
# So wrap modulo 2^64 in native integer arithmetic and mask.
sub i32_mul {
    return (do { use integer; $_[0] * $_[1] }) & 0xFFFFFFFF;
}
