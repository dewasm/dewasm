# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_rename(self, old_dirfd, old_path_ptr, old_path_len, new_dirfd, new_path_ptr, new_path_len):
    old_rel = self.memory.read_string(old_path_ptr, old_path_len).decode("utf-8", "surrogateescape")
    # rename(2) never follows trailing symlinks: it moves the link itself and
    # replaces the destination link.
    old_host, err = self.resolve_path(old_dirfd, old_rel, False)
    if err is not None:
        return err
    new_rel = self.memory.read_string(new_path_ptr, new_path_len).decode("utf-8", "surrogateescape")
    new_host, err = self.resolve_path(new_dirfd, new_rel, False)
    if err is not None:
        return err
    # Trailing-slash semantics (issue #42): resolve_path preserved the slash, so
    # the host rename(2) enforces the uniform shapes itself — an existing
    # non-directory behind a slash is ENOTDIR on either side, a missing
    # slash-bearing source is ENOENT. The one shape Linux and macOS disagree on
    # — a nonexistent slash-bearing destination with a non-directory source
    # (macOS/POSIX ENOENT, Linux ENOTDIR) — is normalized to ENOENT here,
    # mirroring runtime/bash/units/wasi/path_rename.sh. The probes use the
    # slash-stripped path: stat on the slash-bearing one already fails ENOTDIR
    # and would misread "exists as a file" as "missing".
    if new_host.endswith(os.sep):
        old_bare = old_host[:-1] if old_host.endswith(os.sep) else old_host
        new_bare = new_host[:-1]
        # When the source itself bears a slash over an existing non-directory,
        # fall through: the host reports the source-side ENOTDIR first.
        src_slash_ok = not old_host.endswith(os.sep) or os.path.isdir(old_bare)
        if not os.path.lexists(new_bare) and src_slash_ok and not os.path.isdir(old_bare):
            return self.ERRNO_NOENT
    try:
        os.rename(old_host, new_host)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS
