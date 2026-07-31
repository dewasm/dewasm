# requires: memory/i32_load, memory/i32_store, memory/init
sub wasi_fd_read {
    my ($self, $fd, $iovs_ptr, $iovs_len, $nread_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_NOTCAPABLE unless $self->{meta}{$fd}[0] & RIGHTS_FD_READ;
    my $nread = 0;
    for my $i (0 .. $iovs_len - 1) {
        my $ptr = $self->{memory}->i32_load($iovs_ptr + $i * 8);
        my $len = $self->{memory}->i32_load($iovs_ptr + $i * 8 + 4);
        next if $len == 0;
        # sysread is a raw read(2): it returns as soon as any bytes are
        # available — the WASI short-read semantics wasmtime uses — so an
        # interactive tty stdin (the QuickJS REPL under a pty) never
        # deadlocks waiting for a full buffer.
        my $chunk = '';
        my $n = sysread($e->{fh}, $chunk, $len);
        return ERRNO_IO unless defined $n;
        last if $n == 0;
        $self->{memory}->init($ptr, $chunk, 0, $n);
        $nread += $n;
        last if $n < $len;
    }
    $self->{memory}->i32_store($nread_ptr, $nread);
    return ERRNO_SUCCESS;
}
