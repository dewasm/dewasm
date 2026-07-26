// requires: memory/_class
// The bundled WASI preview 1 runtime (ADR-29; filesystem model ADR-14, adopted
// one-for-one from the Ruby/Python backends). Stdio and files are *os.File; a
// directory — whether a preopen or one the guest opened via path_open — is a
// *wasiDir. The fd table holds either, keyed by fd. Args/env are pre-encoded
// byte strings; env is passed already-ordered ("K=V") and preopens are assigned
// fds in sorted order, so there is no map-iteration nondeterminism (ADR-29).
const (
    wasiOk    uint32 = 0
    wasiBadf  uint32 = 8
    wasiInval uint32 = 28
    wasiIo    uint32 = 29
    wasiNosys uint32 = 52
    wasiSpipe uint32 = 70
)

// A directory descriptor (ADR-14): either a preopen (preopenName set to the
// guest-visible path passed in preopens) or a directory the guest opened itself
// via path_open (preopenName nil). entries is the fd_readdir listing cache,
// filled lazily; loaded guards the one-shot snapshot.
type wasiDir struct {
    hostPath    string
    preopenName []byte
    entries     []wasiDirent
    loaded      bool
}

type wasiDirent struct {
    name     []byte
    filetype byte
}

type WASI struct {
    args   [][]byte
    env    [][]byte
    fds    map[uint32]any
    nextFd uint32
    memory *Memory
}

func newWASI(args []string, env []string, preopens map[string]string) *WASI {
    w := &WASI{fds: map[uint32]any{0: os.Stdin, 1: os.Stdout, 2: os.Stderr}}
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
        info, err := os.Stat(real)
        if err != nil || !info.IsDir() {
            panic("preopen " + guest + " => " + preopens[guest] + ": not a directory")
        }
        w.fds[nextFd] = &wasiDir{hostPath: real, preopenName: []byte(guest)}
        nextFd++
    }
    w.nextFd = nextFd
    return w
}

// isStdio reports whether f is one of the three inherited standard streams,
// which take the SPIPE/no-close special cases (in lockstep with fds 0..2).
func (w *WASI) isStdio(f *os.File) bool {
    return f == os.Stdin || f == os.Stdout || f == os.Stderr
}
