# requires: memory/i64_load8_u, rt/sext
sub i64_load8_s {
    my ($self, $a) = @_;
    return Rt::sext($self->i64_load8_u($a), 8, 0xFFFFFFFFFFFFFFFF);
}
