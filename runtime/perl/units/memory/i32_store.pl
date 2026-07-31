sub i32_store {
    my ($self, $a, $v) = @_;
    $self->check($a, 4);
    substr($self->{data}, $a, 4, pack('V', $v & 0xFFFFFFFF));
}
