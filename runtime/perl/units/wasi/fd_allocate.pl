# requires: wasi/errno_fs
sub wasi_fd_allocate {
    my ($self, $fd, $offset, $len) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    # fallocate never shrinks: grow the file to offset+len when that
    # exceeds the current size, otherwise leave it untouched.
    my $size = (stat($e->{fh}))[7];
    return $self->fs_errno(0 + $!) unless defined $size;
    if ($offset + $len > $size) {
        truncate($e->{fh}, $offset + $len) or return $self->fs_errno(0 + $!);
    }
    return ERRNO_SUCCESS;
}
