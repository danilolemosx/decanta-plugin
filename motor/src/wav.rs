//! Leitura e escrita de WAV (RIFF) com o chunk bext (Broadcast Wave).

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};

/// Formato das amostras no arquivo de origem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formato {
    /// PCM inteiro com o número de bits indicado (8, 16, 24 ou 32).
    Inteiro(u16),
    /// Ponto flutuante IEEE 32-bit.
    Float32,
    /// Ponto flutuante IEEE 64-bit.
    Float64,
}

impl Formato {
    pub fn descricao(&self) -> String {
        match self {
            Formato::Inteiro(b) => format!("PCM {b}-bit"),
            Formato::Float32 => "float 32-bit".into(),
            Formato::Float64 => "float 64-bit".into(),
        }
    }
}

/// Áudio lido de um WAV.
#[derive(Debug, Clone)]
pub struct Wav {
    pub sr: u32,
    pub formato: Formato,
    /// Amostras por canal, normalizadas por 2^(bits-1) (exato em f32 para 8/16/24-bit).
    pub canais: Vec<Vec<f32>>,
    /// time_reference do chunk bext (amostras desde a meia-noite), se houver.
    pub time_reference: Option<u64>,
    /// description do chunk bext, se houver.
    pub descricao: Option<String>,
}

impl Wav {
    pub fn n_amostras(&self) -> usize {
        self.canais.first().map_or(0, |c| c.len())
    }
}

const GUID_SUFIXO: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

/// Lê um WAV PCM (8/16/24/32-bit) ou float (32/64-bit), incluindo WAVE_FORMAT_EXTENSIBLE e bext.
pub fn ler(caminho: &Path) -> Result<Wav> {
    let dados = fs::read(caminho).with_context(|| format!("não foi possível ler {}", caminho.display()))?;
    ler_bytes(&dados).with_context(|| format!("WAV inválido: {}", caminho.display()))
}

pub fn ler_bytes(dados: &[u8]) -> Result<Wav> {
    if dados.len() < 12 || &dados[0..4] != b"RIFF" || &dados[8..12] != b"WAVE" {
        bail!("não é um arquivo RIFF/WAVE");
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16, u16)> = None; // (tag, canais, sr, block_align, bits)
    let mut audio: Option<&[u8]> = None;
    let mut time_reference = None;
    let mut descricao = None;
    while pos + 8 <= dados.len() {
        let id = &dados[pos..pos + 4];
        let tam = u32le(dados, pos + 4) as usize;
        let ini = pos + 8;
        // chunk data truncado (ou tamanho 0xFFFFFFFF): usar até o fim do arquivo
        let fim = ini.saturating_add(tam).min(dados.len());
        let corpo = &dados[ini..fim];
        match id {
            b"fmt " => {
                if corpo.len() < 16 {
                    bail!("chunk fmt curto");
                }
                let mut tag = u16le(corpo, 0);
                if tag == 0xFFFE && corpo.len() >= 26 {
                    tag = u16le(corpo, 24);
                }
                fmt = Some((tag, u16le(corpo, 2), u32le(corpo, 4), u16le(corpo, 12), u16le(corpo, 14)));
            }
            b"bext" if corpo.len() >= 346 => {
                let lo = u32le(corpo, 338) as u64;
                let hi = u32le(corpo, 342) as u64;
                time_reference = Some(lo | (hi << 32));
                let d = &corpo[..256];
                let n = d.iter().position(|&c| c == 0).unwrap_or(256);
                descricao = Some(String::from_utf8_lossy(&d[..n]).into_owned());
            }
            b"data" => audio = Some(corpo),
            _ => {}
        }
        pos = ini.saturating_add(tam).saturating_add(tam & 1);
    }
    let (tag, n_ch, sr, block_align, bits) = fmt.context("chunk fmt ausente")?;
    let audio = audio.context("chunk data ausente")?;
    let n_ch = n_ch as usize;
    if n_ch == 0 {
        bail!("número de canais inválido");
    }
    let formato = match (tag, bits) {
        (1, 8 | 16 | 24 | 32) => Formato::Inteiro(bits),
        (3, 32) => Formato::Float32,
        (3, 64) => Formato::Float64,
        _ => bail!("formato não suportado (tag {tag}, {bits} bits)"),
    };
    let bps = (bits / 8) as usize;
    let quadro = (block_align as usize).max(bps * n_ch);
    let n = audio.len() / quadro;
    let mut canais = vec![Vec::with_capacity(n); n_ch];
    for q in 0..n {
        for (c, canal) in canais.iter_mut().enumerate() {
            let i = q * quadro + c * bps;
            let b = &audio[i..i + bps];
            let v = match formato {
                Formato::Inteiro(8) => (b[0] as f32 - 128.0) / 128.0,
                Formato::Inteiro(16) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
                Formato::Inteiro(24) => {
                    (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0
                }
                Formato::Inteiro(_) => {
                    (i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 / 2_147_483_648.0) as f32
                }
                Formato::Float32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                Formato::Float64 => f64::from_le_bytes(b.try_into().unwrap()) as f32,
            };
            canal.push(v);
        }
    }
    Ok(Wav { sr, formato, canais, time_reference, descricao })
}

/// Converte uma amostra float em PCM 24-bit exatamente como o ffmpeg do protocolo
/// (swresample flt→s32: `clip(llrintf(x * 2^31))`, depois pcm_s24le: `v >> 8`).
pub fn amostra_24bit(x: f32) -> i32 {
    let v = (x as f64 * 2_147_483_648.0).round_ties_even();
    let v = v.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    v >> 8
}

/// Converte uma amostra float em PCM 16-bit como o ffmpeg (flt→s16: `clip(lrintf(x * 2^15))`).
pub fn amostra_16bit(x: f32) -> i16 {
    (x as f64 * 32_768.0).round_ties_even().clamp(i16::MIN as f64, i16::MAX as f64) as i16
}

/// Grava WAV mono PCM 24-bit com fmt (WAVE_FORMAT_EXTENSIBLE, como o ffmpeg), bext e data.
/// Nenhum outro chunk/metadado é escrito.
pub fn gravar_24bit_bext(
    caminho: &Path,
    amostras: &[f32],
    sr: u32,
    time_reference: u64,
    descricao: &str,
) -> Result<()> {
    gravar_bext(caminho, &[amostras], 24, sr, time_reference, descricao)
}

/// Grava WAV PCM (16 ou 24 bits, 1 ou mais canais de mesmo tamanho) com chunks fmt, bext e data.
/// Como o ffmpeg: 24-bit ou mais de 2 canais usam WAVE_FORMAT_EXTENSIBLE; 16-bit mono/estéreo, PCM simples.
pub fn gravar_bext(
    caminho: &Path,
    canais: &[&[f32]],
    bits: u16,
    sr: u32,
    time_reference: u64,
    descricao: &str,
) -> Result<()> {
    if canais.is_empty() || !(bits == 16 || bits == 24) {
        bail!("formato de gravação inválido");
    }
    let n_ch = canais.len();
    let n = canais[0].len();
    if canais.iter().any(|c| c.len() != n) {
        bail!("canais com tamanhos diferentes");
    }
    let bps = (bits / 8) as usize;
    let quadro = bps * n_ch;
    let extensivel = bits > 16 || n_ch > 2;
    let mut fmt = Vec::with_capacity(40);
    fmt.extend_from_slice(&(if extensivel { 0xFFFEu16 } else { 1 }).to_le_bytes());
    fmt.extend_from_slice(&(n_ch as u16).to_le_bytes());
    fmt.extend_from_slice(&sr.to_le_bytes());
    fmt.extend_from_slice(&(sr * quadro as u32).to_le_bytes()); // bytes/s
    fmt.extend_from_slice(&(quadro as u16).to_le_bytes()); // block align
    fmt.extend_from_slice(&bits.to_le_bytes());
    if extensivel {
        let mascara: u32 = match n_ch {
            1 => 4, // centro
            2 => 3, // esquerda + direita
            _ => 0,
        };
        fmt.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        fmt.extend_from_slice(&bits.to_le_bytes()); // bits válidos
        fmt.extend_from_slice(&mascara.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes()); // subformato PCM
        fmt.extend_from_slice(&GUID_SUFIXO);
    }

    let mut bext = vec![0u8; 602];
    let d = descricao.as_bytes();
    let nd = d.len().min(256);
    bext[..nd].copy_from_slice(&d[..nd]);
    bext[338..346].copy_from_slice(&time_reference.to_le_bytes());
    bext[346..348].copy_from_slice(&1u16.to_le_bytes()); // versão BWF 1

    let data_len = n * quadro;
    let riff_len = 4 + (8 + fmt.len()) + (8 + bext.len()) + (8 + data_len + (data_len & 1));
    if riff_len > u32::MAX as usize {
        bail!("arquivo grande demais para WAV (> 4 GB)");
    }
    let mut out = Vec::with_capacity(riff_len + 8);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    for (id, corpo) in [(b"fmt ", &fmt), (b"bext", &bext)] {
        out.extend_from_slice(id);
        out.extend_from_slice(&(corpo.len() as u32).to_le_bytes());
        out.extend_from_slice(corpo);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..n {
        for c in canais {
            if bits == 24 {
                out.extend_from_slice(&amostra_24bit(c[i]).to_le_bytes()[..3]);
            } else {
                out.extend_from_slice(&amostra_16bit(c[i]).to_le_bytes());
            }
        }
    }
    if data_len & 1 == 1 {
        out.push(0);
    }
    let mut f = fs::File::create(caminho)
        .with_context(|| format!("não foi possível criar {}", caminho.display()))?;
    f.write_all(&out)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn conversao_24bit_como_ffmpeg() {
        assert_eq!(amostra_24bit(0.0), 0);
        assert_eq!(amostra_24bit(1.0), 8_388_607); // satura
        assert_eq!(amostra_24bit(-1.0), -8_388_608);
        assert_eq!(amostra_24bit(2.0), 8_388_607);
        assert_eq!(amostra_24bit(-3.0), -8_388_608);
        // valores int16/32768 são exatos em 24-bit
        assert_eq!(amostra_24bit(-1632.0 / 32768.0), -1632 * 256);
        assert_eq!(amostra_24bit(12345.0 / 32768.0), 12345 * 256);
        // arredondamento a 32-bit e depois deslocamento aritmético (piso)
        assert_eq!(amostra_24bit(-1e-9), -1);
        assert_eq!(amostra_24bit(1e-9), 0);
    }

    #[test]
    fn grava_e_le_24bit_com_bext() {
        let dir = std::env::temp_dir().join(format!("motor-teste-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        let x: Vec<f32> = (0..1001).map(|i| ((i as f32) * 0.01).sin() * 0.5).collect();
        gravar_24bit_bext(&p, &x, 48_000, 173_106_934, "teste DF leve").unwrap();
        let w = ler(&p).unwrap();
        assert_eq!(w.sr, 48_000);
        assert_eq!(w.formato, Formato::Inteiro(24));
        assert_eq!(w.n_amostras(), 1001);
        assert_eq!(w.time_reference, Some(173_106_934));
        assert_eq!(w.descricao.as_deref(), Some("teste DF leve"));
        for (a, b) in x.iter().zip(&w.canais[0]) {
            assert_eq!(amostra_24bit(*a) as f32 / 8_388_608.0, *b);
        }
        // só fmt, bext e data
        let bytes = fs::read(&p).unwrap();
        let mut pos = 12;
        let mut ids = Vec::new();
        while pos + 8 <= bytes.len() {
            ids.push(String::from_utf8_lossy(&bytes[pos..pos + 4]).into_owned());
            let t = u32le(&bytes, pos + 4) as usize;
            pos += 8 + t + (t & 1);
        }
        assert_eq!(ids, ["fmt ", "bext", "data"]);
        assert_eq!(bytes.len() % 2, 0);
        fs::remove_dir_all(dir).ok();
    }
}
