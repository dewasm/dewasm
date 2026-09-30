# requires: memory/fill, memory/i32_store8, memory/i32_store16, memory/i64_store
sub wasi_fd_fdstat_get {
    my ($self, $fd, $out_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF unless defined $e;
    # `-t` is a host system call.
    # An open descriptor's `filetype` cannot change while it is open.
    # So it runs at most once per `fd`, and the answer is memoized in the entry of the `fd` table.
    # `fd_renumber` moves that entry and `fd_close` drops it.
    # A guest calling `isatty` in a loop would otherwise pay one system call per call.
    my $filetype = $e->{filetype} //= $e->{dir} ? 3 : (-t $e->{fh} ? 2 : 4);  # directory / char device / regular file
    my ($base, $inheriting, $fdflags) = @{$self->{meta}{$fd}};
    # `fdstat`: `fs_filetype` (u8) + padding + `fs_flags` (u16) + padding + `fs_rights_base` (u64)
    # + `fs_rights_inheriting` (u64) = 24 bytes.
    $self->{memory}->fill($out_ptr, 0, 24);
    $self->{memory}->i32_store8($out_ptr, $filetype);
    $self->{memory}->i32_store16($out_ptr + 2, $fdflags);
    $self->{memory}->i64_store($out_ptr + 8, $base);
    $self->{memory}->i64_store($out_ptr + 16, $inheriting);
    return ERRNO_SUCCESS;
}
