# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_symlink(self, old_path_ptr, old_path_len, fd, new_path_ptr, new_path_len):
    target = self.memory.read_string(old_path_ptr, old_path_len).decode("utf-8", "surrogateescape")
    # The symlink target is stored verbatim (never pre-resolved); containment is enforced when the link is later followed.
    # An absolute target can never be confined to the preopen, so it is rejected up front.
    if target.startswith("/"):
        return self.ERRNO_NOTCAPABLE
    new_rel = self.memory.read_string(new_path_ptr, new_path_len).decode("utf-8", "surrogateescape")
    link_host, err = self.resolve_path(fd, new_rel, False)
    if err is not None:
        return err
    # Slash-suffixed link name, per wasmtime: EEXIST on a directory,
    # ENOTDIR on a non-directory (raw Linux would say EEXIST), ENOENT when missing.
    # Probe the slash-stripped path.
    if link_host.endswith(os.sep):
        bare = link_host[:-1]
        if os.path.lexists(bare):
            return self.ERRNO_EXIST if os.path.isdir(bare) else self.ERRNO_NOTDIR
        return self.ERRNO_NOENT
    try:
        os.symlink(target, link_host)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS
