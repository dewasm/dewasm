// Decode a chunked Base64 constant into one byte[].
// A data segment can exceed the 64KB Java string-literal limit, so the emitter splits it into chunks each under the limit and this rejoins them.
// Element/data blobs are never emitted as raw byte lists.
static byte[] data_from_b64(String[] chunks) {
    java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
    java.util.Base64.Decoder dec = java.util.Base64.getDecoder();
    for (String c : chunks) {
        byte[] part = dec.decode(c);
        out.write(part, 0, part.length);
    }
    return out.toByteArray();
}
