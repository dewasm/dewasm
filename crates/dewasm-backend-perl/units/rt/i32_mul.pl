# An i32 product can exceed 2^53, where plain perl arithmetic goes through
# NVs and loses precision; wrap mod 2^64 in native integer arithmetic and mask.
sub i32_mul {
    return (do { use integer; $_[0] * $_[1] }) & 0xFFFFFFFF;
}
