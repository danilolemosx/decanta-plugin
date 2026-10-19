//! Compressor da voz (só na fala já separada, para o fundo não subir junto).
//!
//! Compressor feed-forward com joelho suave, detecção de pico ligada entre os canais (a mesma
//! redução em todos, para não deslocar a imagem estéreo) e ataque/soltura no domínio do ganho.
//!
//! O botão "Compressão" (0 a 100%) move threshold e ratio juntos, pensado para diálogo; em modo
//! manual valem os controles completos. Nos dois casos o volume é compensado (makeup) para que
//! uma fala em −18 dBFS saia no mesmo nível.

/// Ajustes do compressor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamsCompressor {
    /// Botão Compressão, 0 a 1 (0 = desligado).
    pub quantidade: f32,
    /// Usar os controles completos em vez do botão único.
    pub manual: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
}

impl Default for ParamsCompressor {
    fn default() -> Self {
        ParamsCompressor {
            quantidade: 0.0,
            manual: false,
            threshold_db: -22.0,
            ratio: 3.0,
            attack_ms: 8.0,
            release_ms: 150.0,
            knee_db: 6.0,
        }
    }
}

/// Nível de referência da fala para o makeup automático.
const REFERENCIA_DB: f32 = -18.0;

/// Valores efetivos: (threshold, ratio, attack, release, knee).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Efetivos {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
}

impl ParamsCompressor {
    /// Desligado? (botão em 0 no modo simples, ou ratio 1 no manual)
    pub fn desligado(&self) -> bool {
        if self.manual {
            self.ratio <= 1.0
        } else {
            self.quantidade <= 0.0
        }
    }

    pub fn efetivos(&self) -> Efetivos {
        if self.manual {
            Efetivos {
                threshold_db: self.threshold_db,
                ratio: self.ratio.max(1.0),
                attack_ms: self.attack_ms.max(0.1),
                release_ms: self.release_ms.max(1.0),
                knee_db: self.knee_db.max(0.0),
            }
        } else {
            let q = self.quantidade.clamp(0.0, 1.0);
            Efetivos { threshold_db: -12.0 - 24.0 * q, ratio: 1.0 + 5.0 * q, attack_ms: 8.0, release_ms: 150.0, knee_db: 6.0 }
        }
    }
}

/// Curva estática: ganho (dB, ≤ 0) para um nível de entrada `x` (dB).
pub fn ganho_estatico(x: f32, e: &Efetivos) -> f32 {
    let (t, r, w) = (e.threshold_db, e.ratio, e.knee_db);
    let d = x - t;
    let y = if 2.0 * d < -w {
        x
    } else if w > 0.0 && 2.0 * d.abs() <= w {
        x + (1.0 / r - 1.0) * (d + w / 2.0).powi(2) / (2.0 * w)
    } else {
        t + d / r
    };
    (y - x).min(0.0)
}

/// Ganho de compensação (dB): devolve uma fala em −18 dBFS ao mesmo nível.
pub fn makeup(e: &Efetivos) -> f32 {
    -ganho_estatico(REFERENCIA_DB, e)
}

pub struct Compressor {
    sr: f32,
    /// Redução atual (dB, ≤ 0), suavizada.
    reducao: f32,
}

impl Compressor {
    pub fn novo(sr: f32) -> Self {
        Compressor { sr: sr.max(1.0), reducao: 0.0 }
    }

    pub fn reiniciar(&mut self) {
        self.reducao = 0.0;
    }

    /// Comprime os canais no lugar. Devolve a maior redução do bloco (dB, ≤ 0).
    pub fn processar(&mut self, mut canais: Vec<&mut [f32]>, p: &ParamsCompressor) -> f32 {
        if p.desligado() || canais.is_empty() {
            self.reducao = 0.0;
            return 0.0;
        }
        let e = p.efetivos();
        let ca = (-1.0 / (e.attack_ms * 1e-3 * self.sr)).exp();
        let cr = (-1.0 / (e.release_ms * 1e-3 * self.sr)).exp();
        let mk = makeup(&e);
        let n = canais[0].len();
        let mut maior = 0f32;
        for i in 0..n {
            let pico = canais.iter().fold(0f32, |m, c| m.max(c[i].abs()));
            let nivel = if pico > 1e-9 { 20.0 * pico.log10() } else { -180.0 };
            let alvo = ganho_estatico(nivel, &e);
            // descendo (mais redução) = ataque; subindo = soltura
            let c = if alvo < self.reducao { ca } else { cr };
            self.reducao = alvo + c * (self.reducao - alvo);
            maior = maior.min(self.reducao);
            let g = 10f32.powf((self.reducao + mk) / 20.0);
            for ch in canais.iter_mut() {
                ch[i] *= g;
            }
        }
        maior
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn curva_estatica() {
        let e = Efetivos { threshold_db: -20.0, ratio: 4.0, attack_ms: 1.0, release_ms: 1.0, knee_db: 0.0 };
        assert_eq!(ganho_estatico(-30.0, &e), 0.0);
        assert!((ganho_estatico(-8.0, &e) - (-9.0)).abs() < 1e-5); // 12 dB acima → sai 3 dB acima
        // joelho suave: contínuo e só reduz
        let k = Efetivos { knee_db: 6.0, ..e };
        assert!((ganho_estatico(-23.0, &k) - 0.0).abs() < 1e-6);
        assert!(ganho_estatico(-20.0, &k) < 0.0 && ganho_estatico(-20.0, &k) > -1.2);
        assert!((ganho_estatico(-8.0, &k) - (-9.0)).abs() < 1e-5);
    }

    #[test]
    fn desligado_nao_altera() {
        let mut c = Compressor::novo(48_000.0);
        let mut x = vec![0.9f32; 100];
        let r = c.processar(vec![&mut x[..]], &ParamsCompressor::default());
        assert_eq!(r, 0.0);
        assert!(x.iter().all(|&v| v == 0.9));
    }

    #[test]
    fn reduz_picos_e_compensa_fala_media() {
        let p = ParamsCompressor { quantidade: 0.5, ..Default::default() };
        let e = p.efetivos();
        assert!(makeup(&e) > 0.0);
        let mut c = Compressor::novo(48_000.0);
        // fala alta (−6 dBFS) por 1 s: a redução tem de chegar perto da curva estática
        let a = 10f32.powf(-6.0 / 20.0);
        let mut x: Vec<f32> = (0..48_000).map(|i| if i % 2 == 0 { a } else { -a }).collect();
        let r = c.processar(vec![&mut x[..]], &p);
        assert!((r - ganho_estatico(-6.0, &e)).abs() < 0.5, "redução {r}");
        // a saída do pico fica abaixo da entrada (comprimiu mais do que compensou)
        assert!(x[47_999].abs() < a);
    }
}
