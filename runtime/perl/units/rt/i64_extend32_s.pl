# requires: rt/sext
sub i64_extend32_s {
    return Rt::sext($_[0], 32, 0xFFFFFFFFFFFFFFFF);
}
