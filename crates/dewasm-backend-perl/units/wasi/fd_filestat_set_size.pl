# requires:
sub wasi_fd_filestat_set_size {
    my ($self, $fd, $size) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_NOTCAPABLE unless $self->{meta}{$fd}[0] & RIGHTS_FD_FILESTAT_SET_SIZE;
    return ERRNO_IO unless truncate($e->{fh}, $size);
    return ERRNO_SUCCESS;
}
