# requires:
sub wasi_fd_advise {
    my ($self, $fd, $offset, $len, $advice) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    # posix_fadvise is purely advisory; validating the fd and succeeding is
    # a faithful no-op.
    return ERRNO_SUCCESS;
}
