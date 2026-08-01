# requires: memory/i32_store
sub wasi_args_sizes_get {
    my ($self, $argc_ptr, $buf_size_ptr) = @_;
    my $bytes = 0;
    $bytes += length($_) + 1 for @{$self->{args}};
    $self->{memory}->i32_store($argc_ptr, scalar @{$self->{args}});
    $self->{memory}->i32_store($buf_size_ptr, $bytes);
    return ERRNO_SUCCESS;
}
