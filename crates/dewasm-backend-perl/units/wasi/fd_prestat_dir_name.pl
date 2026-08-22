# requires: memory/init, wasi/errno_fs
sub wasi_fd_prestat_dir_name {
    my ($self, $fd, $path_ptr, $path_len) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF unless defined($e) && $e->{dir} && defined($e->{preopen});
    my $name = $e->{preopen};
    return ERRNO_NAMETOOLONG if length($name) > $path_len;
    $self->{memory}->init($path_ptr, $name, 0, length($name));
    return ERRNO_SUCCESS;
}
