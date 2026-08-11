# requires: memory/i32_load, memory/i32_store, memory/read_string
sub wasi_fd_write {
    my ($self, $fd, $iovs_ptr, $iovs_len, $nwritten_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_NOTCAPABLE unless $self->{meta}{$fd}[0] & RIGHTS_FD_WRITE;
    # fdflags::APPEND is honoured here (not via O_APPEND on the OS handle)
    # so fd_fdstat_set_flags(0) can turn it back off.
    if (($self->{meta}{$fd}[2] & 0x1) && !defined($e->{std})) {
        return ERRNO_IO unless defined sysseek($e->{fh}, 0, 2);
    }
    my $written = 0;
    for my $i (0 .. $iovs_len - 1) {
        my $ptr = $self->{memory}->i32_load($iovs_ptr + $i * 8);
        my $len = $self->{memory}->i32_load($iovs_ptr + $i * 8 + 4);
        next if $len == 0;
        my $chunk = $self->{memory}->read_string($ptr, $len);
        my $n = syswrite($e->{fh}, $chunk);
        return ERRNO_IO unless defined $n;
        $written += $n;
        # A single write(2) may write short; stop so the reported nwritten
        # stays contiguous.
        last if $n < $len;
    }
    $self->{memory}->i32_store($nwritten_ptr, $written);
    return ERRNO_SUCCESS;
}
