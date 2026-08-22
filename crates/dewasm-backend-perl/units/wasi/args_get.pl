# requires: wasi/write_string_list
sub wasi_args_get {
    my ($self, $argv_ptr, $buf_ptr) = @_;
    return $self->write_string_list($self->{args}, $argv_ptr, $buf_ptr);
}
