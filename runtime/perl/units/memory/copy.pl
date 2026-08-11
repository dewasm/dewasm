# 4-arg substr copies the extracted source before replacing, so overlapping ranges are safe.
sub copy {
    my ($self, $dst, $src, $len) = @_;
    $self->check($dst, $len);
    $self->check($src, $len);
    return if $len == 0;
    substr($self->{data}, $dst, $len, substr($self->{data}, $src, $len));
}
