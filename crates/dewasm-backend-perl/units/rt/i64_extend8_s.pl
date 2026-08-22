# requires: rt/sext
sub i64_extend8_s {
    return Rt::sext($_[0], 8, 0xFFFFFFFFFFFFFFFF);
}
