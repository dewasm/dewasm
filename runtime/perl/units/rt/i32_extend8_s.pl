# requires: rt/sext
sub i32_extend8_s {
    return Rt::sext($_[0], 8, 0xFFFFFFFF);
}
