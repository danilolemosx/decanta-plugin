//! Espectrograma para verificação visual, com os parâmetros do protocolo:
//! NFFT 1024, overlap 768, janela Hann, PSD em dB (como matplotlib `specgram`),
//! faixa de -130 a -30 dB, até 12 kHz, colormap magma.

use anyhow::Result;
use rustfft::{num_complex::Complex, FftPlanner};

use crate::magma::MAGMA;

pub const NFFT: usize = 1024;
pub const OVERLAP: usize = 768;
pub const DB_MIN: f64 = -130.0;
pub const DB_MAX: f64 = -30.0;
pub const FREQ_MAX: f64 = 12_000.0;

/// Matriz de PSD em dB: `colunas[t][f]`, f de 0 Hz até `FREQ_MAX`.
pub struct Espectrograma {
    pub colunas: Vec<Vec<f32>>,
    pub sr: u32,
}

/// Calcula o espectrograma reduzindo o eixo do tempo para no máximo `largura` colunas
/// (média da potência dos quadros que caem em cada coluna).
pub fn calcular(x: &[f32], sr: u32, largura: usize) -> Espectrograma {
    let hop = NFFT - OVERLAP;
    let n_quadros = if x.len() >= NFFT { (x.len() - NFFT) / hop + 1 } else { 0 };
    let n_bins = ((FREQ_MAX * NFFT as f64 / sr as f64).floor() as usize + 1).min(NFFT / 2 + 1);
    // Hann simétrica (np.hanning, usada por mlab.window_hanning)
    let janela: Vec<f64> = (0..NFFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (NFFT - 1) as f64).cos())
        .collect();
    let soma_w2: f64 = janela.iter().map(|w| w * w).sum();
    let largura = largura.max(1).min(n_quadros.max(1));
    let mut acum = vec![vec![0f64; n_bins]; largura];
    let mut conta = vec![0usize; largura];
    let fft = FftPlanner::<f64>::new().plan_fft_forward(NFFT);
    let mut buf = vec![Complex::new(0.0, 0.0); NFFT];
    for q in 0..n_quadros {
        let ini = q * hop;
        for (i, b) in buf.iter_mut().enumerate() {
            *b = Complex::new(x[ini + i] as f64 * janela[i] + 1e-9 * janela[i], 0.0);
        }
        fft.process(&mut buf);
        let col = q * largura / n_quadros.max(1);
        for (f, a) in acum[col].iter_mut().enumerate() {
            // PSD unilateral com scale_by_freq (mlab): |X|^2 / (fs * sum(w^2)), dobrado fora de DC/Nyquist
            let mut p = buf[f].norm_sqr() / (sr as f64 * soma_w2);
            if f != 0 && f != NFFT / 2 {
                p *= 2.0;
            }
            *a += p;
        }
        conta[col] += 1;
    }
    let colunas = acum
        .into_iter()
        .zip(conta)
        .map(|(c, n)| c.into_iter().map(|p| (10.0 * (p / n.max(1) as f64).max(1e-30).log10()) as f32).collect())
        .collect();
    Espectrograma { colunas, sr }
}

impl Espectrograma {
    /// Imagem RGB (largura × altura), frequências altas no topo.
    pub fn rgb(&self) -> (usize, usize, Vec<u8>) {
        let w = self.colunas.len();
        let h = self.colunas.first().map_or(0, |c| c.len());
        let mut px = vec![0u8; w * h * 3];
        for (x, col) in self.colunas.iter().enumerate() {
            for (f, &db) in col.iter().enumerate() {
                let t = ((db as f64 - DB_MIN) / (DB_MAX - DB_MIN)).clamp(0.0, 1.0);
                let c = MAGMA[((t * 256.0) as usize).min(255)];
                let y = h - 1 - f;
                px[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&c);
            }
        }
        (w, h, px)
    }

    pub fn png(&self) -> Result<Vec<u8>> {
        let (w, h, px) = self.rgb();
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(&px)?;
        }
        Ok(out)
    }
}
