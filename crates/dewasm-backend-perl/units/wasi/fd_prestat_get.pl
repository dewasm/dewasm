# requires: memory/init
sub wasi_fd_prestat_get {
    my ($self, $fd, $out_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF unless defined($e) && $e->{dir} && defined($e->{preopen});
    # prestat: tag (u8, 0 = dir) + 3 pad + pr_name_len (u32).
    $self->{memory}->init($out_ptr, pack('CxxxV', 0, length($e->{preopen})), 0, 8);
    return ERRNO_SUCCESS;
}
