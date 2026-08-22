# requires: rt/trap
# Also used to initialize active data segments at instantiation time.
sub init {
    my ($self, $dst, $data, $src, $len) = @_;
    Rt::trap('out of bounds memory access') if $src + $len > length($data);
    $self->check($dst, $len);
    return if $len == 0;
    substr($self->{data}, $dst, $len, substr($data, $src, $len));
}
