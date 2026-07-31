sub read_string {
    my ($self, $ptr, $len) = @_;
    $self->check($ptr, $len);
    return substr($self->{data}, $ptr, $len);
}
