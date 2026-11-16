"""Writes a plain 1024x1024 orange RGBA icon.png (no dependencies). Replace with your own art any time."""
import struct, zlib

W = H = 1024
raw = (b"\x00" + bytes([0xFF, 0x6B, 0x35, 0xFF]) * W) * H

def chunk(tag, data):
    body = tag + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

png = (
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(raw, 9))
    + chunk(b"IEND", b"")
)
open("icon.png", "wb").write(png)
print("wrote icon.png", len(png), "bytes")
