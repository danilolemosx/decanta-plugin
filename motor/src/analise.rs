//! Inspeção: amostras clipadas, silêncio digital e verificação de atraso por correlação.

use rustfft::{num_complex::Complex, FftPlanner};

/// Limiar de clip do protocolo: |x| >= 0,999.
pub const LIMIAR_CLIP: f32 = 0.999;
/// Silêncio digital é reportado quando dura mais que isto (segundos).
pub const SILENCIO_MIN_S: f64 = 0.5;

/// Trecho de silêncio digital (amostras exatamente iguais a zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Silencio {
    pub inicio: usize,
    pub duracao: usize,
}

pub fn contar_clip(x: &[f32]) -> usize {
    x.iter().filter(|v| v.abs() >= LIMIAR_CLIP).count()
}

/// Trechos de zeros digitais com mais de `SILENCIO_MIN_S` segundos.
pub fn silencios(x: &[f32], sr: u32) -> Vec<Silencio> {
    let minimo = SILENCIO_MIN_S * sr as f64;
    let mut out = Vec::new();
    let mut inicio = None;
    for (i, &v) in x.iter().chain(std::iter::once(&1.0)).enumerate() {
        match (v == 0.0, inicio) {
            (true, None) => inicio = Some(i),
            (false, Some(s)) => {
                if (i - s) as f64 > minimo {
                    out.push(Silencio { inicio: s, duracao: i - s });
                }
                inicio = None;
            }
            _ => {}
        }
    }
    out
}

/// Atraso (em amostras) de `b` em relação a `a` que maximiza a correlação cruzada,
/// usando no máximo os primeiros `max_amostras`. 0 = alinhados.
pub fn atraso_por_correlacao(a: &[f32], b: &[f32], max_amostras: usize) -> i64 {
    let m = a.len().min(b.len()).min(max_amostras);
    if m == 0 {
        return 0;
    }
    let nfft = (2 * m).next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(nfft);
    let inv = planner.plan_fft_inverse(nfft);
    let carregar = |x: &[f32]| {
        let mut v: Vec<Complex<f64>> = x[..m].iter().map(|&s| Complex::new(s as f64, 0.0)).collect();
        v.resize(nfft, Complex::new(0.0, 0.0));
        fwd.process(&mut v);
        v
    };
    let fa = carregar(a);
    let fb = carregar(b);
    let mut cc: Vec<Complex<f64>> = fa.iter().zip(&fb).map(|(x, y)| x * y.conj()).collect();
    inv.process(&mut cc);
    // cc[k] = sum a[n+k] b[n]; índices altos são atrasos negativos
    let mut melhor = (f64::MIN, 0i64);
    for k in 0..nfft {
        let lag = if k < m { k as i64 } else if k > nfft - m { k as i64 - nfft as i64 } else { continue };
        if cc[k].re > melhor.0 {
            melhor = (cc[k].re, lag);
        }
    }
    -melhor.1
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn clip_e_silencio() {
        let mut x = vec![0.1f32; 48_000 * 5];
        x[10] = 0.999;
        x[11] = -1.0;
        x[12] = 0.998;
        for v in &mut x[48_000..48_000 + 24_001] {
            *v = 0.0; // 0,5 s + 1 amostra: reportar
        }
        for v in &mut x[100_000..100_000 + 24_000] {
            *v = 0.0; // exatamente 0,5 s: não reportar
        }
        let n = x.len();
        for v in &mut x[n - 30_000..] {
            *v = 0.0; // no fim do arquivo
        }
        assert_eq!(contar_clip(&x), 2);
        assert_eq!(
            silencios(&x, 48_000),
            vec![Silencio { inicio: 48_000, duracao: 24_001 }, Silencio { inicio: n - 30_000, duracao: 30_000 }]
        );
    }

    #[test]
    fn atraso() {
        let a: Vec<f32> = (0..20_000).map(|i| ((i * 7919 % 1000) as f32 / 500.0) - 1.0).collect();
        assert_eq!(atraso_por_correlacao(&a, &a, 48_000), 0);
        let mut b = vec![0.0; 37];
        b.extend_from_slice(&a[..a.len() - 37]);
        assert_eq!(atraso_por_correlacao(&a, &b, 48_000), 37);
        assert_eq!(atraso_por_correlacao(&b, &a, 48_000), -37);
    }
}
