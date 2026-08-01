sub i64_store16 {
    my ($self, $a, $v) = @_;
    $self->check($a, 2);
    substr($self->{data}, $a, 2, pack('v', $v & 0xFFFF));
}
