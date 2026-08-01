# requires: wasi/write_string_list
sub wasi_environ_get {
    my ($self, $environ_ptr, $buf_ptr) = @_;
    return $self->write_string_list($self->{env}, $environ_ptr, $buf_ptr);
}
