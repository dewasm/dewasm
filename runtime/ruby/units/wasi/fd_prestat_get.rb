def wasi_fd_prestat_get(_fd, _out_ptr)
  ERRNO_BADF # no preopened directories yet
end
