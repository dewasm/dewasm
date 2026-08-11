# requires: memory/i32_load, memory/i32_store, memory/init
sub wasi_fd_pread {
    my ($self, $fd, $iovs_ptr, $iovs_len, $offset, $nread_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_SPIPE if defined $e->{std};
    return ERRNO_NOTCAPABLE unless $self->{meta}{$fd}[0] & RIGHTS_FD_READ;
    # Core perl has no pread(2): emulate with a save/seek/read/restore on the unbuffered (sysread/sysseek-only) handle, which stays coherent.
    my $cur = sysseek($e->{fh}, 0, 1);
    return ERRNO_IO unless defined $cur;
    my $nread = 0;
    for my $i (0 .. $iovs_len - 1) {
        my $ptr = $self->{memory}->i32_load($iovs_ptr + $i * 8);
        my $len = $self->{memory}->i32_load($iovs_ptr + $i * 8 + 4);
        next if $len == 0;
        my $chunk = '';
        my $n;
        if (defined sysseek($e->{fh}, $offset + $nread, 0)) {
            $n = sysread($e->{fh}, $chunk, $len);
        }
        if (!defined $n) {
            sysseek($e->{fh}, $cur, 0);
            return ERRNO_IO;
        }
        last if $n == 0;
        $self->{memory}->init($ptr, $chunk, 0, $n);
        $nread += $n;
        last if $n < $len;
    }
    sysseek($e->{fh}, $cur, 0);
    $self->{memory}->i32_store($nread_ptr, $nread);
    return ERRNO_SUCCESS;
}
