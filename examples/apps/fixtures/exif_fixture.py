#!/usr/bin/env python3
# exif_fixture.py: first-party generator for the exiftool e2e image fixture.
#
# Hand-assembles a tiny (~200 byte) JPEG whose only real content is an EXIF
# APP1 segment carrying three deterministic tags (Make, Model, and
# DateTimeOriginal), so the exiftool-on-zeroperl case can pin an exact output string.
# Everything is fixed, so the bytes are reproducible.
#
# The scan payload is a single grey pixel's worth of placeholder marker segments: ExifTool only needs SOI + the APP1 EXIF block + EOI to identify the file and extract metadata, but a bare SOI/APP1/EOI trips its "looks like trailer garbage" heuristics, so we include a minimal (empty) SOS run.
#
# Regenerate (writes examples/apps/fixtures/exif_fixture.jpg) with:
# python3 examples/apps/fixtures/exif_fixture.py
# Committing the resulting binary is intentional (the repo already commits binary snapshots); the sha is stable across runs.

import struct
from pathlib import Path

# --- EXIF tags, little-endian ("II") TIFF.
# Values longer than 4 bytes live in a data area after the IFD, referenced by a TIFF-relative offset.
MAKE = b"DewasmCam\x00"
MODEL = b"Model-X\x00"
DATETIME = b"2020:01:02 03:04:05\x00"

TYPE_ASCII = 2
TYPE_LONG = 4


def entry(tag, typ, count, value):
    return struct.pack("<HHII", tag, typ, count, value)


# Layout offsets are all relative to the start of the TIFF header.
ifd0_off = 8
ifd0_size = 2 + 3 * 12 + 4  # count + 3 entries + next-IFD pointer
make_off = ifd0_off + ifd0_size
model_off = make_off + len(MAKE)
exififd_off = model_off + len(MODEL)
exififd_size = 2 + 1 * 12 + 4  # count + 1 entry + next-IFD pointer
datetime_off = exififd_off + exififd_size

tiff = bytearray()
tiff += b"II" + struct.pack("<HI", 0x2A, ifd0_off)  # header -> IFD0 at 8

# IFD0: Make, Model, and a pointer to the Exif sub-IFD (tags must ascend).
tiff += struct.pack("<H", 3)
tiff += entry(0x010F, TYPE_ASCII, len(MAKE), make_off)
tiff += entry(0x0110, TYPE_ASCII, len(MODEL), model_off)
tiff += entry(0x8769, TYPE_LONG, 1, exififd_off)  # ExifOffset
tiff += struct.pack("<I", 0)  # no IFD1
tiff += MAKE + MODEL

# Exif sub-IFD: DateTimeOriginal only.
tiff += struct.pack("<H", 1)
tiff += entry(0x9003, TYPE_ASCII, len(DATETIME), datetime_off)
tiff += struct.pack("<I", 0)
tiff += DATETIME

exif = b"Exif\x00\x00" + bytes(tiff)
app1 = b"\xFF\xE1" + struct.pack(">H", len(exif) + 2) + exif

jpeg = bytearray()
jpeg += b"\xFF\xD8"  # SOI
jpeg += app1
# A minimal 1x1 baseline frame so the file reads as a real JPEG, not a stub.
jpeg += b"\xFF\xDB\x00\x43\x00" + bytes([1] * 64)  # DQT (all-ones table)
jpeg += b"\xFF\xC0\x00\x0B\x08\x00\x01\x00\x01\x01\x01\x11\x00"  # SOF0 1x1
jpeg += b"\xFF\xC4\x00\x14\x00\x01" + bytes([0] * 15 + [0])  # DHT (minimal)
jpeg += b"\xFF\xDA\x00\x08\x01\x01\x00\x00\x3F\x00"  # SOS header
jpeg += b"\x00"  # one byte of entropy-coded data
jpeg += b"\xFF\xD9"  # EOI

out = Path(__file__).resolve().parent / "exif_fixture.jpg"
out.write_bytes(bytes(jpeg))
print(f"wrote {out} ({len(jpeg)} bytes)")
