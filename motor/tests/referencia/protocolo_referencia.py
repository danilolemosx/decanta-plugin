#!/usr/bin/env python3
"""Protocolo de referência "Tratamento de áudio 2", exatamente como aprovado em produção.

Usa o binário deep-filter v0.5.6, numpy e ffmpeg (só para a exportação final, como no protocolo).
Uso: protocolo_referencia.py <deep-filter> <original.wav> <pasta_saida>
Grava em <pasta_saida> os 4 WAVs 24-bit com bext e imprime um resumo.
"""
import os
import struct
import subprocess
import sys
import tempfile

import numpy as np

NIVEIS = [(12, "leve"), (20, "medio"), (100, "forte")]


def ler_wav(caminho):
    """Lê WAV PCM 16/24/32-bit ou float32. Retorna (float64 [amostras, canais], sr, time_reference)."""
    with open(caminho, "rb") as f:
        dados = f.read()
    assert dados[:4] == b"RIFF" and dados[8:12] == b"WAVE"
    pos, fmt, audio, tr = 12, None, None, None
    while pos + 8 <= len(dados):
        cid, tam = dados[pos : pos + 4], struct.unpack("<I", dados[pos + 4 : pos + 8])[0]
        corpo = dados[pos + 8 : pos + 8 + tam]
        if cid == b"fmt ":
            tag, ch, sr, _, _, bits = struct.unpack("<HHIIHH", corpo[:16])
            if tag == 0xFFFE:
                tag = struct.unpack("<H", corpo[24:26])[0]
            fmt = (tag, ch, sr, bits)
        elif cid == b"bext":
            lo, hi = struct.unpack("<II", corpo[338:346])
            tr = lo | (hi << 32)
        elif cid == b"data":
            audio = corpo
        pos += 8 + tam + (tam & 1)
    tag, ch, sr, bits = fmt
    if tag == 3:
        x = np.frombuffer(audio, "<f4").astype(np.float64)
    elif bits == 16:
        x = np.frombuffer(audio, "<i2") / 32768.0
    elif bits == 24:
        b = np.frombuffer(audio, np.uint8).reshape(-1, 3).astype(np.int32)
        v = b[:, 0] | (b[:, 1] << 8) | (b[:, 2] << 16)
        v = np.where(v >= 1 << 23, v - (1 << 24), v)
        x = v / 8388608.0
    elif bits == 32:
        x = np.frombuffer(audio, "<i4") / 2147483648.0
    return x.reshape(-1, ch), sr, tr


def gravar_f32(caminho, x, sr):
    x = x.astype("<f4")
    fmt = struct.pack("<HHIIHH", 3, 1, sr, sr * 4, 4, 32)
    data = x.tobytes()
    with open(caminho, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", 4 + 8 + 16 + 8 + len(data)) + b"WAVE")
        f.write(b"fmt " + struct.pack("<I", 16) + fmt)
        f.write(b"data" + struct.pack("<I", len(data)) + data)


def main():
    df, original, saida = sys.argv[1:4]
    os.makedirs(saida, exist_ok=True)
    nome = os.path.splitext(os.path.basename(original))[0]
    orig, sr, tr = ler_wav(original)
    assert sr == 48000 and orig.shape[1] == 1, "referência: só mono 48 kHz"
    orig = orig[:, 0]
    n = len(orig)
    print(f"original: {n} amostras, time_reference={tr}")
    print(f"clip (|x|>=0.999): {int(np.sum(np.abs(orig) >= 0.999))}")

    tmp = tempfile.mkdtemp()
    sinais = {}
    for nivel, rotulo in NIVEIS:
        out = os.path.join(tmp, f"df_{nivel}")
        subprocess.run([df, "-D", "-a", str(nivel), "-o", out, original], check=True)
        y = ler_wav(os.path.join(out, os.path.basename(original)))[0][:, 0]
        # ler_wav já divide int16 por 32768; completar com zeros no FIM
        print(f"{rotulo}: saída do binário com {len(y)} amostras ({n - len(y)} a menos)")
        y = np.concatenate([y, np.zeros(n - len(y))])
        sinais[rotulo] = y
    # conferir atraso 0 por correlação nos primeiros 20 s
    m = min(n, 20 * sr)
    a, b = orig[:m], sinais["forte"][:m]
    nfft = 1 << int(np.ceil(np.log2(2 * m)))
    cc = np.fft.irfft(np.fft.rfft(a, nfft) * np.conj(np.fft.rfft(b, nfft)), nfft)
    lag = int(np.argmax(np.concatenate([cc[-(m - 1) :], cc[:m]]))) - (m - 1)
    print(f"atraso por correlação (20 s): {lag}")
    sinais["passos_e_ambiente_removidos"] = orig - sinais["forte"]

    for rotulo, y in sinais.items():
        tmpf = os.path.join(tmp, f"{rotulo}.wav")
        gravar_f32(tmpf, y, sr)
        destino = os.path.join(saida, f"{nome}_DF_{rotulo}.wav")
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y", "-i", tmpf, "-map_metadata", "-1", "-c:a", "pcm_s24le",
             "-write_bext", "1", "-metadata", f"time_reference={tr}",
             "-metadata", f"description={nome} DF {rotulo}", destino],
            check=True,
        )
        y2, _, tr2 = ler_wav(destino)
        assert len(y2) == n and tr2 == tr, destino
        print(f"gravado {os.path.basename(destino)}")


if __name__ == "__main__":
    main()
