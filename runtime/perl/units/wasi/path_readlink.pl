# requires: memory/read_string, memory/init, memory/i32_store, wasi/resolve_path, wasi/errno_fs
sub wasi_path_readlink {
    my ($self, $fd, $path_ptr, $path_len, $buf_ptr, $buf_len, $bufused_ptr) = @_;
    my $rel = $self->{memory}->read_string($path_ptr, $path_len);
    my ($host, $err) = $self->resolve_path($fd, $rel, 0);
    return $err if defined $err;
    my $target = readlink($host);
    return $self->fs_errno(0 + $!) unless defined $target;
    # The content is truncated to buf_len (no NUL terminator); bufused reports how many bytes were written.
    my $n = length($target) < $buf_len ? length($target) : $buf_len;
    $self->{memory}->init($buf_ptr, $target, 0, $n);
    $self->{memory}->i32_store($bufused_ptr, $n);
    return ERRNO_SUCCESS;
}
