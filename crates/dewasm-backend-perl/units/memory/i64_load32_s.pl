# requires: memory/i64_load32_u, rt/sext
sub i64_load32_s {
    my ($self, $a) = @_;
    return Rt::sext($self->i64_load32_u($a), 32, 0xFFFFFFFFFFFFFFFF);
}
