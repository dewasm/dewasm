sub i64_store {
    my ($self, $a, $v) = @_;
    $self->check($a, 8);
    substr($self->{data}, $a, 8, pack('Q<', $v & 0xFFFFFFFFFFFFFFFF));
}
