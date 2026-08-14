// requires: rt/exception, rt/trap
func (rt) throw_ref(exn *rtException) {
    if exn == nil {
        Rt.trap("null exception reference")
    }
    panic(exn)
}
