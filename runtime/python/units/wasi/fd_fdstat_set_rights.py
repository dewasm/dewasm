def wasi_fd_fdstat_set_rights(self, fd, fs_rights_base, fs_rights_inheriting):
    meta = self.fd_meta.get(fd)
    if meta is None or fd not in self.fds:
        return self.ERRNO_BADF
    # Rights can only be narrowed, never re-granted: a request for any bit the
    # fd does not already hold is NOTCAPABLE (ADR-40).
    if (fs_rights_base & ~meta[0]) or (fs_rights_inheriting & ~meta[1]):
        return self.ERRNO_NOTCAPABLE
    meta[0] = fs_rights_base
    meta[1] = fs_rights_inheriting
    return self.ERRNO_SUCCESS
