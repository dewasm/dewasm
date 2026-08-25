// requires: memory/_class
// The bundled WASI preview 1 runtime (filesystem model adopted one-for-one from the Ruby/Python backends).
// Stdio and files are *os.File; a directory (whether a preopen or one the guest opened via path_open) is a
// *wasiDir.
// The fd table holds either, keyed by fd.
// Args/env are pre-encoded byte strings; env is passed already-ordered ("K=V") and preopens are assigned fds in sorted order, so there is no map-iteration nondeterminism.
const (
    wasiOk         uint32 = 0
    wasiBadf       uint32 = 8
    wasiInval      uint32 = 28
    wasiIo         uint32 = 29
    wasiNosys      uint32 = 52
    wasiSpipe      uint32 = 70
    wasiNotcapable uint32 = 76 // rights-narrowing violation
)

// WASI p1 rights bits (per-fd rights model adopted from the reference runtime's per-filetype masks).
// A preopen/dir fd carries dirRightsBase; a file fd carries whatever path_open requested intersected with the dir's inheriting set.
// Enforced NOTCAPABLE=76 in the fd_read/fd_write/fd_seek/fd_readdir/ fd_filestat_set_size units and narrowed by fd_fdstat_set_rights.
const (
    rightFdDatasync          uint64 = 1 << 0
    rightFdRead              uint64 = 1 << 1
    rightFdSeek              uint64 = 1 << 2
    rightFdFdstatSetFlags    uint64 = 1 << 3
    rightFdSync              uint64 = 1 << 4
    rightFdTell              uint64 = 1 << 5
    rightFdWrite             uint64 = 1 << 6
    rightFdAdvise            uint64 = 1 << 7
    rightFdAllocate          uint64 = 1 << 8
    rightPathCreateDirectory uint64 = 1 << 9
    rightPathCreateFile      uint64 = 1 << 10
    rightPathLinkSource      uint64 = 1 << 11
    rightPathLinkTarget      uint64 = 1 << 12
    rightPathOpen            uint64 = 1 << 13
    rightFdReaddir           uint64 = 1 << 14
    rightPathReadlink        uint64 = 1 << 15
    rightPathRenameSource    uint64 = 1 << 16
    rightPathRenameTarget    uint64 = 1 << 17
    rightPathFilestatGet     uint64 = 1 << 18
    rightPathFilestatSetSize uint64 = 1 << 19
    rightPathFilestatSetTimes uint64 = 1 << 20
    rightFdFilestatGet       uint64 = 1 << 21
    rightFdFilestatSetSize   uint64 = 1 << 22
    rightFdFilestatSetTimes  uint64 = 1 << 23
    rightPathSymlink         uint64 = 1 << 24
    rightPathRemoveDirectory uint64 = 1 << 25
    rightPathUnlinkFile      uint64 = 1 << 26
    rightPollFdReadwrite     uint64 = 1 << 27

    // The reference runtime's directory masks (what wasi-libc and the conformance suite hard-code as the minimum for every directory).
    // A dir base deliberately excludes FD_SEEK and FD_FILESTAT_SET_SIZE (the suite asserts their absence); inheriting adds the per-file fd_* rights a file opened underneath may request.
    dirRightsBase uint64 = rightPathCreateDirectory | rightPathCreateFile |
        rightPathLinkSource | rightPathLinkTarget | rightPathOpen |
        rightFdReaddir | rightPathReadlink | rightPathRenameSource |
        rightPathRenameTarget | rightPathSymlink | rightPathRemoveDirectory |
        rightPathUnlinkFile | rightPathFilestatGet | rightPathFilestatSetSize |
        rightPathFilestatSetTimes | rightFdFilestatGet | rightFdFilestatSetTimes
    dirRightsInheriting uint64 = dirRightsBase | rightFdDatasync | rightFdRead |
        rightFdSeek | rightFdFdstatSetFlags | rightFdSync | rightFdTell |
        rightFdWrite | rightFdAdvise | rightFdAllocate | rightFdFilestatSetSize |
        rightPollFdReadwrite

    // Stdio streams get a broad tty-shaped set so rights enforcement never blocks the inherited descriptors (seek still answers SPIPE first).
    stdioRights uint64 = rightFdRead | rightFdWrite | rightFdSeek | rightFdTell |
        rightFdFdstatSetFlags | rightFdSync | rightFdDatasync | rightFdAdvise |
        rightFdAllocate | rightFdFilestatGet | rightFdFilestatSetSize |
        rightFdFilestatSetTimes | rightPollFdReadwrite
)

// fdflags bits (fs_flags).
// Only APPEND is acted on (fd_write seeks to end);
// SYNC/DSYNC/RSYNC/NONBLOCK are stored and reported but treated as no-ops.
const (
    fdflagAppend uint16 = 1 << 0
)

// Per-fd rights/flags carried alongside the fd-table entry.
// Every live fd (stdio, preopen, path_open'd) has one; fd_renumber moves it.
// filetype memoizes what fd_fdstat_get reports, valid once filetypeKnown is set.
// An open descriptor's filetype cannot change while it is open, and this meta travels with its fd-table entry (fd_renumber moves both, and fds are never reused after close), so the memoized answer cannot outlive the descriptor it describes.
type wasiFdMeta struct {
    base          uint64
    inheriting    uint64
    fdflags       uint16
    filetype      uint32
    filetypeKnown bool
}

// A directory descriptor: either a preopen (preopenName set to the guest-visible path passed in preopens) or a directory the guest opened itself via path_open (preopenName nil). entries is the fd_readdir listing cache, filled lazily; loaded guards the one-shot snapshot.
type wasiDir struct {
    hostPath    string
    preopenName []byte
    entries     []wasiDirent
    loaded      bool
}

type wasiDirent struct {
    name     []byte
    filetype byte
    ino      uint64
}

type WASI struct {
    args   [][]byte
    env    [][]byte
    fds    map[uint32]any
    meta   map[uint32]*wasiFdMeta
    nextFd uint32
    memory *Memory
}

func newWASI(args []string, env []string, preopens map[string]string) *WASI {
    w := &WASI{
        fds: map[uint32]any{0: os.Stdin, 1: os.Stdout, 2: os.Stderr},
        meta: map[uint32]*wasiFdMeta{
            0: {base: stdioRights, inheriting: stdioRights},
            1: {base: stdioRights, inheriting: stdioRights},
            2: {base: stdioRights, inheriting: stdioRights},
        },
    }
    for _, a := range args {
        w.args = append(w.args, []byte(a))
    }
    for _, e := range env {
        w.env = append(w.env, []byte(e))
    }
    guests := make([]string, 0, len(preopens))
    for g := range preopens {
        guests = append(guests, g)
    }
    sort.Strings(guests)
    nextFd := uint32(3)
    for _, guest := range guests {
        real := preopens[guest]
        if abs, err := filepath.Abs(real); err == nil {
            real = abs
        }
        if resolved, err := filepath.EvalSymlinks(real); err == nil {
            real = resolved
        }
        // The host path must resolve, but need not be a directory: like the
        // Ruby/Perl runtimes, a single-file preopen (e.g. "/dev/null" for the zeroperl reactor's init probe) is accepted: the guest resolves it as the preopen root itself.
        if _, err := os.Stat(real); err != nil {
            panic("preopen " + guest + " => " + preopens[guest] + ": does not exist")
        }
        w.fds[nextFd] = &wasiDir{hostPath: real, preopenName: []byte(guest)}
        w.meta[nextFd] = &wasiFdMeta{base: dirRightsBase, inheriting: dirRightsInheriting}
        nextFd++
    }
    w.nextFd = nextFd
    return w
}

// checkRight reports wasiOk if the fd holds `right`, else wasiNotcapable.
// An fd with no tracked meta (should not happen for a live fd) is permitted.
func (w *WASI) checkRight(fd uint32, right uint64) uint32 {
    if m, ok := w.meta[fd]; ok && m.base&right == 0 {
        return wasiNotcapable
    }
    return wasiOk
}

// isStdio reports whether f is one of the three inherited standard streams, which take the SPIPE/no-close special cases (in lockstep with fds 0..2).
func (w *WASI) isStdio(f *os.File) bool {
    return f == os.Stdin || f == os.Stdout || f == os.Stderr
}
