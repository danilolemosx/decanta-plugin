#!/usr/bin/env python3
"""Compara as saídas do app com as do protocolo de referência.

Uso: comparar.py <pasta_ref> <pasta_app> <nome>
Sai com código 1 se algum arquivo não for idêntico (amostras, time_reference ou chunks fmt/bext/data).
"""
import os
import struct
import sys

import numpy as np

ROTULOS = ["leve", "medio", "forte", "passos_e_ambiente_removidos"]


def chunks(caminho):
    d = open(caminho, "rb").read()
    pos, out = 12, {}
    while pos + 8 <= len(d):
        cid, tam = d[pos : pos + 4], struct.unpack("<I", d[pos + 4 : pos + 8])[0]
        out[cid] = d[pos + 8 : pos + 8 + tam]
        pos += 8 + tam + (tam & 1)
    return out


def amostras24(data):
    b = np.frombuffer(data, np.uint8).reshape(-1, 3).astype(np.int32)
    v = b[:, 0] | (b[:, 1] << 8) | (b[:, 2] << 16)
    return np.where(v >= 1 << 23, v - (1 << 24), v)


def main():
    ref, app, nome = sys.argv[1:4]
    ok = True
    print(f"{'nível':<30} {'amostras ref/app':>21} {'time_ref ref/app':>21} {'amostras dif.':>14} {'dif. máx (LSB 24-bit)':>22} {'dif. máx (dBFS)':>16} {'bytes fmt/bext/data':>20}")
    for r in ROTULOS:
        a = chunks(os.path.join(ref, f"{nome}_DF_{r}.wav"))
        b = chunks(os.path.join(app, f"{nome}_DF_{r}.wav"))
        xa, xb = amostras24(a[b"data"]), amostras24(b[b"data"])
        tra = struct.unpack("<Q", a[b"bext"][338:346])[0]
        trb = struct.unpack("<Q", b[b"bext"][338:346])[0]
        n = min(len(xa), len(xb))
        dif = int(np.max(np.abs(xa[:n] - xb[:n]))) if n else 0
        n_dif = int(np.count_nonzero(xa[:n] != xb[:n]))
        db = "-inf" if dif == 0 else f"{20 * np.log10(dif / 8388608):.1f}"
        iguais = all(a[c] == b[c] for c in (b"fmt ", b"bext", b"data"))
        extra = sorted(set(b) - {b"fmt ", b"bext", b"data"})
        print(f"{r:<30} {len(xa):>10}/{len(xb):<10} {tra:>10}/{trb:<10} {n_dif:>14} {dif:>22} {db:>16} {'idênticos' if iguais else 'DIFERENTES':>20}")
        if extra:
            print(f"   chunks extras no app: {extra}")
        ok &= iguais and len(xa) == len(xb) and tra == trb and dif == 0 and not extra
    print("RESULTADO:", "IDÊNTICO bit a bit à referência" if ok else "DIFERENTE da referência")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
