sub i64_load32_u {
    my ($self, $a) = @_;
    $self->check($a, 4);
    return unpack('V', substr($self->{data}, $a, 4));
}
