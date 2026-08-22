sub fill {
    my ($self, $dst, $val, $len) = @_;
    $self->check($dst, $len);
    return if $len == 0;
    substr($self->{data}, $dst, $len, chr($val & 0xFF) x $len);
}
