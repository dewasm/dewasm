// requires: memory/_class
// The bundled WASI Preview 1 runtime.
// Its file system model is adopted one-for-one from the Ruby/Python backends.
// Stdio and files are *os.File.
// A directory (whether a preopen or one the guest opened via `path_open`) is a `*wasiDir`.
// The file descriptor table holds either, keyed by `fd`.
// The arguments and the environment are pre-encoded byte strings.
// The environment is passed already-ordered ("K=V"), and preopens get `fd`s in sorted order.
// So no result depends on map iteration order.
const (
    wasiOk         uint32 = 0
    wasiBadf       uint32 = 8
    wasiInval      uint32 = 28
    wasiIo         uint32 = 29
    wasiNosys      uint32 = 52
    wasiSpipe      uint32 = 70
    wasiNotcapable uint32 = 76 // rights-narrowing violation
)

// WASI p1 rights bits.
// The rights model per `fd` is adopted from the reference runtime's masks per file type.
// A preopen or directory `fd` carries `dirRightsBase`.
// A file `fd` carries whatever `path_open` requested, masked by the directory's inheriting set.
// Rights are checked (NOTCAPABLE=76) in these units:
// `fd_read`/`fd_write`/`fd_seek`/`fd_readdir`/`fd_filestat_set_size`.
// `fd_fdstat_set_rights` narrows them.
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

    // The reference runtime's directory masks.
    // `wasi-libc` and the conformance suite hard-code them as the minimum for every directory.
    // A directory base deliberately excludes FD_SEEK and FD_FILESTAT_SET_SIZE.
    // The suite asserts their absence.
    // Inheriting adds the per-file `fd_*` rights a file opened underneath may request.
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

    // Stdio streams get a broad set shaped like a TTY's.
    // So rights enforcement never blocks the inherited descriptors.
    // Seek still answers SPIPE first.
    stdioRights uint64 = rightFdRead | rightFdWrite | rightFdSeek | rightFdTell |
        rightFdFdstatSetFlags | rightFdSync | rightFdDatasync | rightFdAdvise |
        rightFdAllocate | rightFdFilestatGet | rightFdFilestatSetSize |
        rightFdFilestatSetTimes | rightPollFdReadwrite
)

// `fdflags` bits (`fs_flags`).
// Only APPEND is acted on (`fd_write` seeks to end).
// SYNC/DSYNC/RSYNC/NONBLOCK are stored and reported but treated as no-ops.
const (
    fdflagAppend uint16 = 1 << 0
)

// Rights and flags per `fd`, carried alongside its entry in the `fd` table.
// Every live `fd` (stdio, preopen, opened by `path_open`) has one; `fd_renumber` moves it.
// `filetype` memoizes what `fd_fdstat_get` reports, valid once `filetypeKnown` is set.
// An open descriptor's file type cannot change while it is open.
// This `wasiFdMeta` travels with its entry in the `fd` table.
// `fd_renumber` moves both, and `fd`s are never reused after close.
// So the memoized answer cannot live longer than the descriptor it describes.
type wasiFdMeta struct {
    base          uint64
    inheriting    uint64
    fdflags       uint16
    filetype      uint32
    filetypeKnown bool
}

// A directory descriptor: either a preopen or a directory the guest opened itself via `path_open`.
// For a preopen, `preopenName` is set to the guest-visible path passed in preopens.
// For a guest-opened directory, `preopenName` is `nil`.
// `entries` is the `fd_readdir` listing cache, filled lazily.
// `loaded` guards the one-shot snapshot.
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
        // The host path must resolve, but need not be a directory.
        // Like the Ruby/Perl runtimes, a single-file preopen is accepted.
        // An example is "/dev/null" for the `zeroperl` reactor's initialization probe.
        // The guest resolves it as the preopen root itself.
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

// `checkRight` reports `wasiOk` if the `fd` holds `right`, else `wasiNotcapable`.
// An `fd` with no tracked `meta` entry (should not happen for a live `fd`) is permitted.
func (w *WASI) checkRight(fd uint32, right uint64) uint32 {
    if m, ok := w.meta[fd]; ok && m.base&right == 0 {
        return wasiNotcapable
    }
    return wasiOk
}

// `isStdio` reports whether `f` is one of the three inherited standard streams.
// Those take the SPIPE/no-close special cases (in step with `fd`s 0..2).
func (w *WASI) isStdio(f *os.File) bool {
    return f == os.Stdin || f == os.Stdout || f == os.Stderr
}
