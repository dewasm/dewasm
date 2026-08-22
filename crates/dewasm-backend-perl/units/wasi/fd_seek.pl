# requires: memory/i64_store, rt/s64
use Errno ();

sub wasi_fd_seek {
    my ($self, $fd, $offset, $whence, $out_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_SPIPE if defined $e->{std};
    return ERRNO_NOTCAPABLE unless $self->{meta}{$fd}[0] & RIGHTS_FD_SEEK;
    return ERRNO_INVAL if $whence > 2;  # WASI whence 0/1/2 = sysseek SET/CUR/END
    my $pos = sysseek($e->{fh}, Rt::s64($offset), $whence);
    if (!defined $pos) {
        # A negative resulting offset is EINVAL, which WASI reports as
        # INVAL (not the generic IO used for other seek failures).
        return 0 + $! == Errno::EINVAL() ? ERRNO_INVAL : ERRNO_IO;
    }
    $self->{memory}->i64_store($out_ptr, $pos);
    return ERRNO_SUCCESS;
}
