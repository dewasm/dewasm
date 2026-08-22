# requires: memory/i32_load, memory/i32_store, memory/read_string
sub wasi_fd_pwrite {
    my ($self, $fd, $iovs_ptr, $iovs_len, $offset, $nwritten_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_SPIPE if defined $e->{std};
    return ERRNO_NOTCAPABLE unless $self->{meta}{$fd}[0] & RIGHTS_FD_WRITE;
    # Core perl has no pwrite(2): emulate with a save/seek/write/restore on the unbuffered handle (see fd_pread).
    my $cur = sysseek($e->{fh}, 0, 1);
    return ERRNO_IO unless defined $cur;
    my $written = 0;
    for my $i (0 .. $iovs_len - 1) {
        my $ptr = $self->{memory}->i32_load($iovs_ptr + $i * 8);
        my $len = $self->{memory}->i32_load($iovs_ptr + $i * 8 + 4);
        next if $len == 0;
        my $chunk = $self->{memory}->read_string($ptr, $len);
        my $n;
        if (defined sysseek($e->{fh}, $offset + $written, 0)) {
            $n = syswrite($e->{fh}, $chunk);
        }
        if (!defined $n) {
            sysseek($e->{fh}, $cur, 0);
            return ERRNO_IO;
        }
        $written += $n;
        # A single pwrite(2) may write short; stop so the reported nwritten stays contiguous.
        last if $n < $len;
    }
    sysseek($e->{fh}, $cur, 0);
    $self->{memory}->i32_store($nwritten_ptr, $written);
    return ERRNO_SUCCESS;
}
