def wasi_fd_renumber(self, fd, to):
    # Both descriptors must currently be open; the destination's own resource is
    # closed, then the source is moved onto it and the source number retired
    # (works onto stdio and preopens too, ADR-40).
    if fd not in self.fds or to not in self.fds:
        return self.ERRNO_BADF
    dst = self.fds[to]
    if not isinstance(dst, self.WasiDir) and dst not in self.std_ios:
        dst.close()
    self.fds[to] = self.fds[fd]
    self.fd_meta[to] = self.fd_meta[fd]
    del self.fds[fd]
    del self.fd_meta[fd]
    return self.ERRNO_SUCCESS
