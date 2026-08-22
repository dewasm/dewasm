# requires: memory/i32_store
sub wasi_environ_sizes_get {
    my ($self, $count_ptr, $buf_size_ptr) = @_;
    my $bytes = 0;
    $bytes += length($_) + 1 for @{$self->{env}};
    $self->{memory}->i32_store($count_ptr, scalar @{$self->{env}});
    $self->{memory}->i32_store($buf_size_ptr, $bytes);
    return ERRNO_SUCCESS;
}
