// Map an os.FileInfo's mode to a WASI filetype tag, using Go's portable
// FileMode bits (so no platform-specific S_IFMT constants are needed).
func (w *WASI) wasi_filetype(fi os.FileInfo) byte {
    m := fi.Mode()
    switch {
    case m&os.ModeDir != 0:
        return 3 // directory
    case m&os.ModeCharDevice != 0:
        return 2 // character device
    case m&os.ModeDevice != 0:
        return 1 // block device
    case m&os.ModeSymlink != 0:
        return 7 // symbolic link
    case m&os.ModeSocket != 0:
        return 6 // socket (stream)
    case m.IsRegular():
        return 4 // regular file
    default:
        return 0 // unknown
    }
}
