"""Genera assets/icon.png y assets/icon.ico (sin dependencias).

Dibujo: cuadrado azul redondeado, arriba un medidor con la aguja verde en
el centro y abajo una octava de teclas de piano.
Uso: python assets/make_icon.py
"""
import math
import os
import struct
import zlib

N = 256
BG = (28, 58, 110)
WHITE = (245, 245, 245)
BLACK = (25, 25, 30)
GREEN = (46, 190, 100)
TICK = (170, 190, 220)


def inside_rounded(x, y, r=48):
    cx = min(max(x, r), N - 1 - r)
    cy = min(max(y, r), N - 1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def pixel(x, y):
    if not inside_rounded(x, y):
        return (0, 0, 0, 0)
    # teclado: y de 140 a 228, x de 28 a 228
    kx0, kx1, ky0, ky1 = 28, 228, 140, 228
    if kx0 <= x < kx1 and ky0 <= y < ky1:
        w = (kx1 - kx0) / 7
        i = (x - kx0) / w
        # teclas negras (despues de C, D, F, G, A)
        for b in (1, 2, 4, 5, 6):
            if abs(i - b) < 0.3 and y < ky0 + 55:
                return BLACK + (255,)
        if (x - kx0) % w < 3:
            return BG + (255,)
        return WHITE + (255,)
    # medidor: arco de marcas y aguja
    cx, cy = 128, 128
    dx, dy = x - cx, cy - y
    dist = math.hypot(dx, dy)
    if dy > 0:
        ang = math.degrees(math.atan2(dx, dy))  # 0 = arriba
        if 82 <= dist <= 100 and abs(ang) <= 60:
            for a in range(-60, 61, 15):
                if abs(ang - a) < 2.2:
                    return TICK + (255,)
        if dist <= 104 and abs(dx) <= 5 and dy > 8:
            return GREEN + (255,)
    if dist <= 12 and y <= cy + 4:
        return GREEN + (255,)
    return BG + (255,)


def png_bytes():
    rows = b"".join(
        b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(N)) for y in range(N)
    )

    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    png = png_bytes()
    with open(os.path.join(here, "icon.png"), "wb") as f:
        f.write(png)
    # ICO con la imagen PNG de 256 adentro (Windows Vista en adelante).
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 6 + 16)
    with open(os.path.join(here, "icon.ico"), "wb") as f:
        f.write(header + entry + png)
    print("ok")


if __name__ == "__main__":
    main()
