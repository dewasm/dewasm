# requires: memory/fill, memory/i32_store8, memory/i32_store16, memory/i64_store
sub wasi_fd_fdstat_get {
    my ($self, $fd, $out_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF unless defined $e;
    my $filetype = $e->{dir} ? 3 : (-t $e->{fh} ? 2 : 4);  # directory / char device / regular file
    my ($base, $inheriting, $fdflags) = @{$self->{meta}{$fd}};
    # fdstat: fs_filetype (u8) + pad + fs_flags (u16) + pad + fs_rights_base
    # (u64) + fs_rights_inheriting (u64) = 24 bytes.
    $self->{memory}->fill($out_ptr, 0, 24);
    $self->{memory}->i32_store8($out_ptr, $filetype);
    $self->{memory}->i32_store16($out_ptr + 2, $fdflags);
    $self->{memory}->i64_store($out_ptr + 8, $base);
    $self->{memory}->i64_store($out_ptr + 16, $inheriting);
    return ERRNO_SUCCESS;
}
