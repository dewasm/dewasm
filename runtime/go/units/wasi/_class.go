// requires: memory/_class
// The bundled WASI preview 1 runtime (cowsay milestone: the eight core
// syscalls). Stdio fds map to *os.File; the filesystem model (ADR-14) lands
// with the later milestones. Args/env are pre-encoded byte strings; env is
// passed already-ordered ("K=V") so there is no map-iteration nondeterminism
// (ADR-29).
const (
    wasiOk    uint32 = 0
    wasiBadf  uint32 = 8
    wasiIo    uint32 = 29
    wasiNosys uint32 = 52
)

type WASI struct {
    args   [][]byte
    env    [][]byte
    fds    map[uint32]*os.File
    memory *Memory
}

func newWASI(args []string, env []string) *WASI {
    w := &WASI{fds: map[uint32]*os.File{0: os.Stdin, 1: os.Stdout, 2: os.Stderr}}
    for _, a := range args {
        w.args = append(w.args, []byte(a))
    }
    for _, e := range env {
        w.env = append(w.env, []byte(e))
    }
    return w
}
