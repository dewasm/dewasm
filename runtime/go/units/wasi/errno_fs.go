// Filesystem-only errno codes: kept out of the always-bundled wasi/_class prelude so a stdio-only WASI module (no path_* / fs-only fd_* imports) doesn't carry them.
// The base codes (BADF/INVAL/IO/SPIPE) live in
// _class since stdio needs them too.
const (
    wasiAcces       uint32 = 2
    wasiExist       uint32 = 20
    wasiIsdir       uint32 = 31
    wasiLoop        uint32 = 32
    wasiNametoolong uint32 = 37
    wasiNoent       uint32 = 44
    wasiNotdir      uint32 = 54
    wasiNotempty    uint32 = 55
    wasiPerm        uint32 = 63
    // wasiNotcapable (76) lives in wasi/_class: the per-fd rights model needs it even in stdio-only modules that don't bundle these fs-only codes.
)

// One host-error-to-WASI-errno mapping shared by every filesystem syscall, so the same host error never maps to different codes depending on which syscall raised it.
// Go wraps the raw errno in *fs.PathError/*os.LinkError, so unwrap it with errors.As.
func (w *WASI) fs_errno(err error) uint32 {
    var errno syscall.Errno
    if errors.As(err, &errno) {
        switch errno {
        case syscall.EACCES:
            return wasiAcces
        case syscall.EBADF:
            return wasiBadf
        case syscall.EEXIST:
            return wasiExist
        case syscall.EINVAL:
            return wasiInval
        case syscall.EISDIR:
            return wasiIsdir
        case syscall.ELOOP:
            return wasiLoop
        case syscall.ENAMETOOLONG:
            return wasiNametoolong
        case syscall.ENOENT:
            return wasiNoent
        case syscall.ENOTDIR:
            return wasiNotdir
        case syscall.ENOTEMPTY:
            return wasiNotempty
        case syscall.EPERM:
            return wasiPerm
        case syscall.ESPIPE:
            return wasiSpipe
        }
    }
    return wasiIo
}
