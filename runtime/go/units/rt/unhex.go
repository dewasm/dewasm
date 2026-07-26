// Decode a lowercase-hex string into bytes. Data/element blobs are emitted as
// hex literals (never raw byte lists) so they compile fast and cannot hide a
// package-selector substring from the import scanner (ADR-29).
func (rt) unhex(s string) []byte {
    b := make([]byte, len(s)/2)
    for i := 0; i < len(b); i++ {
        var hi, lo byte
        c := s[i*2]
        if c <= '9' {
            hi = c - '0'
        } else {
            hi = c - 'a' + 10
        }
        c = s[i*2+1]
        if c <= '9' {
            lo = c - '0'
        } else {
            lo = c - 'a' + 10
        }
        b[i] = hi<<4 | lo
    }
    return b
}
