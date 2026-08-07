// A function table for call_indirect: a fixed-size array of funcref slots
// (null = uninitialized). Populated by active element segments at
// instantiation.
Rt.Funcref[] slots;

Table(int size) {
    this.slots = new Rt.Funcref[size];
}
