# requires:
# Any open fd is accepted, stdio included: toywasm's WASI setup sets NONBLOCK on stdin and treats failure as fatal, so wasmtime's regular-files-only EBADF answer is not copied.
sub wasi_fd_fdstat_set_flags {
    my ($self, $fd, $flags) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    # Store the new fdflags; fd_write consults fdflags::APPEND before each write so clearing it here (set_flags 0) actually turns append off.
    $self->{meta}{$fd}[2] = $flags & 0xFFFF;
    return ERRNO_SUCCESS;
}
