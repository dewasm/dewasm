# requires: memory/i64_load16_u, rt/sext
sub i64_load16_s {
    my ($self, $a) = @_;
    return Rt::sext($self->i64_load16_u($a), 16, 0xFFFFFFFFFFFFFFFF);
}
