# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
sub wasi_path_symlink {
    my ($self, $old_path_ptr, $old_path_len, $fd, $new_path_ptr, $new_path_len) = @_;
    my $target = $self->{memory}->read_string($old_path_ptr, $old_path_len);
    # The symlink target is stored verbatim (never pre-resolved);
    # containment is enforced when the link is later followed. An
    # absolute target can never be confined to the preopen, so it is
    # rejected up front.
    return ERRNO_NOTCAPABLE if rindex($target, '/', 0) == 0;
    my $new_rel = $self->{memory}->read_string($new_path_ptr, $new_path_len);
    my ($link_host, $err) = $self->resolve_path($fd, $new_rel, 0);
    return $err if defined $err;
    # Slash-suffixed link name, per wasmtime: EEXIST on a
    # directory, ENOTDIR on a non-directory (raw Linux would say EEXIST),
    # ENOENT when missing. Probe the slash-stripped path.
    if (substr($link_host, -1) eq '/') {
        my $bare = substr($link_host, 0, -1);
        if (lstat($bare)) {
            return -d $bare ? ERRNO_EXIST : ERRNO_NOTDIR;
        }
        return ERRNO_NOENT;
    }
    symlink($target, $link_host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
