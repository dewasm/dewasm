# requires:
# Any open `fd` is accepted, stdio included.
# The WASI set-up of `toywasm` sets NONBLOCK on `stdin` and treats failure as fatal.
# So Wasmtime's EBADF answer for anything but a regular file is not copied.
sub wasi_fd_fdstat_set_flags {
    my ($self, $fd, $flags) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    # Store the new `fdflags`; `fd_write` consults `fdflags::APPEND` before each write.
    # So clearing it here (`set_flags 0`) actually turns append off.
    $self->{meta}{$fd}[2] = $flags & 0xFFFF;
    return ERRNO_SUCCESS;
}
