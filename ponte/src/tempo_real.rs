//! Modo tempo real (Premiere, efeito de faixa): trata o áudio em blocos de qualquer tamanho
//! enquanto toca.
//!
//! Por canal: o DeepFilterNet trabalha em quadros de 10 ms (480 amostras a 48 kHz). A saída sai
//! atrasada de `LATENCIA` amostras (1 quadro de espera + o atraso do modelo), que o programa
//! compensa. O original passa por uma linha de atraso igual, para o fundo (original − voz)
//! ficar alinhado.
//!
//! Caminho do som: separação → voz (compressor → ganho → volume) + fundo (volume) → limitador.
//! Com tudo no padrão (voz 0 dB, fundo desligado, compressão 0, ganho 0), a voz é exatamente a
//! saída do DeepFilterNet do app, antes da etapa int16 do protocolo.
//!
//! Em outras taxas (44,1, 96 kHz...) o áudio é convertido para 48 kHz na entrada e de volta na
//! saída, em blocos fixos; o atraso total continua exato e informado ao programa.

use std::collections::VecDeque;

use anyhow::{anyhow, bail, Result};
use rubato::{FftFixedInOut, Resampler};
use df::tract::{DfParams, DfTract, ReduceMask, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};

use crate::compressor::{Compressor, ParamsCompressor};

/// Taxa do modelo; as outras são convertidas.
pub const SR: u32 = 48_000;
/// Quadro do DeepFilterNet a 48 kHz.
pub const HOP: usize = 480;
/// Atraso total da saída: um quadro de espera + o atraso do modelo (fft − hop + lookahead·hop).
pub const LATENCIA: usize = HOP + 1440;
/// Abaixo disto o volume é considerado desligado (−∞).
pub const DB_MUDO: f32 = -60.0;

/// Ajustes que podem mudar enquanto toca.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ajustes {
    /// Separação: limite de atenuação do DeepFilterNet (0 a 100 dB).
    pub separacao_db: f32,
    pub pos_filtro: f32,
    pub limiar_ruido_db: f32,
    /// Volume da voz (fala limpa), em dB; ≤ −60 = desligado.
    pub voz_db: f32,
    /// Volume do fundo retirado, em dB; ≤ −60 = desligado.
    pub ambiencia_db: f32,
    pub compressor: ParamsCompressor,
    /// Ganho da voz depois do compressor, em dB.
    pub ganho_db: f32,
    /// Ouvir só o fundo retirado (sem volume nem compressor).
    pub so_fundo: bool,
}

impl Ajustes {
    /// Algum ajuste aumenta o volume? (ganho, voz ou fundo acima de 0 dB, ou compressor ligado)
    pub fn aumenta_volume(&self) -> bool {
        self.ganho_db > 0.0 || self.voz_db > 0.0 || self.ambiencia_db > 0.0 || !self.compressor.desligado()
    }
}

impl Default for Ajustes {
    fn default() -> Self {
        Ajustes {
            separacao_db: 20.0,
            pos_filtro: 0.0,
            limiar_ruido_db: -15.0,
            voz_db: 0.0,
            ambiencia_db: DB_MUDO,
            compressor: ParamsCompressor::default(),
            ganho_db: 0.0,
            so_fundo: false,
        }
    }
}

/// Medidores do último bloco (pico, em amplitude linear; redução do compressor em dB ≤ 0).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct Medidores {
    pub entrada: f32,
    pub saida: f32,
    pub reducao_db: f32,
}

fn db_para_ganho(db: f32) -> f32 {
    if db <= DB_MUDO {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

fn novo_modelo(aj: &Ajustes) -> Result<DfTract> {
    let mut rp = RuntimeParams::default()
        .with_atten_lim(aj.separacao_db)
        .with_thresholds(aj.limiar_ruido_db, 35., 35.)
        .with_mask_reduce(ReduceMask::MAX);
    if aj.pos_filtro > 0.0 {
        rp = rp.with_post_filter(aj.pos_filtro);
    }
    DfTract::new(DfParams::default(), &rp).map_err(|e| anyhow!("falha ao carregar o DeepFilterNet: {e}"))
}

struct Canal {
    modelo: DfTract,
    /// Entrada ainda sem quadro completo.
    entrada: VecDeque<f32>,
    /// Voz já separada, esperando para sair (começa com um quadro de silêncio).
    voz: VecDeque<f32>,
    /// Original atrasado de LATENCIA, para alinhar com a voz.
    original: VecDeque<f32>,
    quadro_in: Vec<f32>,
    quadro_out: Vec<f32>,
}

impl Canal {
    fn novo(aj: &Ajustes) -> Result<Self> {
        let mut c = Canal {
            modelo: novo_modelo(aj)?,
            entrada: VecDeque::with_capacity(4 * HOP),
            voz: VecDeque::with_capacity(8 * HOP),
            original: VecDeque::with_capacity(LATENCIA + 8 * HOP),
            quadro_in: vec![0.0; HOP],
            quadro_out: vec![0.0; HOP],
        };
        c.zerar_filas();
        Ok(c)
    }

    fn zerar_filas(&mut self) {
        self.entrada.clear();
        self.voz.clear();
        self.voz.extend(std::iter::repeat(0.0).take(HOP));
        self.original.clear();
        self.original.extend(std::iter::repeat(0.0).take(LATENCIA));
    }

    fn aplicar(&mut self, aj: &Ajustes) {
        self.modelo.set_atten_lim(aj.separacao_db);
        self.modelo.set_pf_beta(aj.pos_filtro);
        self.modelo.min_db_thresh = aj.limiar_ruido_db;
    }

    /// Separa `x`: devolve em `voz` e `original` as amostras atrasadas de LATENCIA.
    fn separar(&mut self, x: &[f32], voz: &mut [f32], original: &mut [f32]) -> Result<()> {
        for (i, &v) in x.iter().enumerate() {
            self.entrada.push_back(v);
            self.original.push_back(v);
            if self.entrada.len() == HOP {
                for (d, s) in self.quadro_in.iter_mut().zip(self.entrada.drain(..)) {
                    *d = s;
                }
                let ent = ArrayView2::from_shape((1, HOP), &self.quadro_in[..])?;
                let sai = ArrayViewMut2::from_shape((1, HOP), &mut self.quadro_out[..])?;
                self.modelo.process(ent, sai).map_err(|e| anyhow!("erro no DeepFilterNet: {e}"))?;
                self.voz.extend(self.quadro_out.iter().copied());
            }
            voz[i] = self.voz.pop_front().unwrap_or(0.0);
            original[i] = self.original.pop_front().unwrap_or(0.0);
        }
        Ok(())
    }
}

/// O Decanta a 48 kHz, para `n` canais: separação, compressor e mistura.
struct Nucleo {
    canais: Vec<Canal>,
    compressor: Compressor,
    aplicados: Ajustes,
    /// Ganhos do bloco anterior (para variar suavemente).
    g_voz: f32,
    g_amb: f32,
    buf_voz: Vec<Vec<f32>>,
    buf_orig: Vec<Vec<f32>>,
    buf_fundo: Vec<Vec<f32>>,
}

/// Limitador de proteção: ataque instantâneo (nenhuma amostra passa do teto) e soltura suave.
/// Abaixo do teto não mexe em nada: o som continua idêntico ao do app, que nesse ponto cortaria.
pub struct Limitador {
    ganho: f32,
    coef_soltura: f32,
}

/// Teto da saída (0 dBFS).
pub const TETO: f32 = 1.0;

impl Limitador {
    pub fn novo(sr: f32) -> Self {
        Limitador { ganho: 1.0, coef_soltura: (-1.0 / (0.1 * sr.max(1.0))).exp() }
    }

    pub fn processar(&mut self, canais: &mut [&mut [f32]]) {
        let n = canais.first().map_or(0, |c| c.len());
        for i in 0..n {
            let pico = canais.iter().fold(0f32, |m, c| m.max(c[i].abs()));
            let alvo = if pico > TETO { TETO / pico } else { 1.0 };
            self.ganho = if alvo < self.ganho { alvo } else { alvo + self.coef_soltura * (self.ganho - alvo) };
            if self.ganho < 1.0 {
                // depois do pico, o ganho volta devagar; perto de 1 encosta em 1 de vez
                if self.ganho > 0.999_999 {
                    self.ganho = 1.0;
                }
                for c in canais.iter_mut() {
                    c[i] = (c[i] * self.ganho).clamp(-TETO, TETO);
                }
            }
        }
    }
}

impl Nucleo {
    fn novo(n_canais: usize, aj: &Ajustes) -> Result<Self> {
        let n = n_canais.max(1);
        let canais = (0..n).map(|_| Canal::novo(aj)).collect::<Result<_>>()?;
        Ok(Nucleo {
            canais,
            compressor: Compressor::novo(SR as f32),
            aplicados: *aj,
            g_voz: db_para_ganho(aj.voz_db + aj.ganho_db),
            g_amb: db_para_ganho(aj.ambiencia_db),
            buf_voz: vec![Vec::new(); n],
            buf_orig: vec![Vec::new(); n],
            buf_fundo: vec![Vec::new(); n],
        })
    }

    fn reiniciar(&mut self) -> Result<()> {
        let aj = self.aplicados;
        for c in &mut self.canais {
            c.modelo = novo_modelo(&aj)?;
            c.zerar_filas();
        }
        self.compressor.reiniciar();
        Ok(())
    }

    /// Trata um bloco a 48 kHz. Devolve a redução do compressor (dB, ≤ 0).
    fn processar(&mut self, entradas: &[&[f32]], saidas: &mut [&mut [f32]], aj: &Ajustes) -> Result<f32> {
        let n = entradas.first().map_or(0, |e| e.len());
        if aj.separacao_db != self.aplicados.separacao_db
            || aj.pos_filtro != self.aplicados.pos_filtro
            || aj.limiar_ruido_db != self.aplicados.limiar_ruido_db
        {
            for c in &mut self.canais {
                c.aplicar(aj);
            }
        }
        self.aplicados = *aj;

        // separação, canal por canal
        let n_ch = self.canais.len().min(entradas.len()).min(saidas.len());
        for c in 0..n_ch {
            self.buf_voz[c].resize(n, 0.0);
            self.buf_orig[c].resize(n, 0.0);
            self.canais[c].separar(entradas[c], &mut self.buf_voz[c], &mut self.buf_orig[c])?;
            // o fundo é o que foi retirado: original − voz (antes do compressor)
            let (voz, orig) = (&self.buf_voz[c], &self.buf_orig[c]);
            self.buf_fundo[c].clear();
            self.buf_fundo[c].extend(orig.iter().zip(voz).map(|(&o, &v)| (o as f64 - v as f64) as f32));
        }

        // compressor na voz (canais ligados), depois mistura com o fundo
        let refs: Vec<&mut [f32]> = self.buf_voz[..n_ch].iter_mut().map(|v| v.as_mut_slice()).collect();
        let reducao = if aj.so_fundo { 0.0 } else { self.compressor.processar(refs, &aj.compressor) };

        let g_voz_alvo = db_para_ganho(aj.voz_db + aj.ganho_db);
        let g_amb_alvo = db_para_ganho(aj.ambiencia_db);
        for c in 0..n_ch {
            let (voz, fundo) = (&self.buf_voz[c], &self.buf_fundo[c]);
            let s = &mut saidas[c];
            for i in 0..n {
                let t = if n > 1 { i as f32 / (n - 1) as f32 } else { 1.0 };
                s[i] = if aj.so_fundo {
                    fundo[i]
                } else {
                    let gv = self.g_voz + (g_voz_alvo - self.g_voz) * t;
                    let ga = self.g_amb + (g_amb_alvo - self.g_amb) * t;
                    voz[i] * gv + fundo[i] * ga
                };
            }
        }
        for s in saidas.iter_mut().skip(n_ch) {
            s.fill(0.0);
        }
        self.g_voz = g_voz_alvo;
        self.g_amb = g_amb_alvo;
        Ok(reducao)
    }
}

/// Conversão de taxa em volta do núcleo: blocos fixos de `cin` amostras na taxa do programa
/// viram `cout` amostras a 48 kHz e voltam.
struct Conversao {
    sobe: FftFixedInOut<f32>,
    desce: FftFixedInOut<f32>,
    cin: usize,
    fila_in: Vec<VecDeque<f32>>,
    fila_out: Vec<VecDeque<f32>>,
    b_in: Vec<Vec<f32>>,
    b48: Vec<Vec<f32>>,
    b48_saida: Vec<Vec<f32>>,
    b_out: Vec<Vec<f32>>,
}

impl Conversao {
    fn nova(n: usize, sr: u32) -> Result<Self> {
        // bloco de ~6 ms na taxa do programa (número par de amostras: atraso inteiro)
        let g = gcd(sr as usize, SR as usize);
        let minimo = sr as usize / g;
        let mut k = ((0.006 * sr as f64) / minimo as f64).ceil().max(1.0) as usize;
        if (k * minimo) % 2 == 1 || (k * SR as usize / g) % 2 == 1 {
            k *= 2;
        }
        let sobe = FftFixedInOut::<f32>::new(sr as usize, SR as usize, k * minimo, n)
            .map_err(|e| anyhow!("conversão de taxa: {e}"))?;
        let cin = sobe.input_frames_next();
        let cout = sobe.output_frames_next();
        let desce = FftFixedInOut::<f32>::new(SR as usize, sr as usize, cout, n)
            .map_err(|e| anyhow!("conversão de taxa: {e}"))?;
        if desce.input_frames_next() != cout || desce.output_frames_next() != cin {
            bail!("conversão de taxa: blocos incompatíveis");
        }
        let mut c = Conversao {
            sobe,
            desce,
            cin,
            fila_in: vec![VecDeque::with_capacity(4 * cin); n],
            fila_out: vec![VecDeque::with_capacity(8 * cin); n],
            b_in: vec![vec![0.0; cin]; n],
            b48: vec![vec![0.0; cout]; n],
            b48_saida: vec![vec![0.0; cout]; n],
            b_out: vec![vec![0.0; cin]; n],
        };
        c.zerar();
        Ok(c)
    }

    fn zerar(&mut self) {
        for f in &mut self.fila_in {
            f.clear();
        }
        for f in &mut self.fila_out {
            f.clear();
            f.extend(std::iter::repeat(0.0).take(self.cin));
        }
        self.sobe.reset();
        self.desce.reset();
    }

    /// Atraso total na taxa do programa: espera de um bloco + filtros de ida e volta + núcleo.
    fn latencia(&self, sr: u32) -> usize {
        let ida = self.sobe.output_delay() as f64 * sr as f64 / SR as f64;
        let volta = self.desce.output_delay() as f64;
        let nucleo = LATENCIA as f64 * sr as f64 / SR as f64;
        self.cin + (ida + nucleo + volta).round() as usize
    }
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Um Decanta em tempo real, para `n` canais, em qualquer taxa de 8 a 192 kHz.
pub struct TempoReal {
    nucleo: Nucleo,
    conversao: Option<Conversao>,
    limitador: Limitador,
    sr: u32,
    latencia: usize,
}

impl TempoReal {
    pub fn novo(n_canais: usize, sr: u32, aj: &Ajustes) -> Result<Self> {
        if !(8_000..=192_000).contains(&sr) {
            bail!("taxa não suportada: {sr} Hz");
        }
        let n = n_canais.max(1);
        let conversao = if sr == SR { None } else { Some(Conversao::nova(n, sr)?) };
        let latencia = conversao.as_ref().map_or(LATENCIA, |c| c.latencia(sr));
        Ok(TempoReal { nucleo: Nucleo::novo(n, aj)?, conversao, limitador: Limitador::novo(sr as f32), sr, latencia })
    }

    /// Trata o áudio? (sempre, entre 8 e 192 kHz)
    pub fn ativo(&self) -> bool {
        true
    }

    /// Atraso da saída, em amostras na taxa do programa.
    pub fn latencia(&self) -> usize {
        self.latencia
    }

    pub fn taxa(&self) -> u32 {
        self.sr
    }

    /// Volta ao estado inicial (play em outro ponto, por exemplo).
    pub fn reiniciar(&mut self) -> Result<()> {
        self.nucleo.reiniciar()?;
        if let Some(c) = &mut self.conversao {
            c.zerar();
        }
        self.limitador = Limitador::novo(self.sr as f32);
        Ok(())
    }

    /// Trata um bloco. `entradas[c]` e `saidas[c]` têm `n` amostras (podem ser o mesmo buffer).
    pub fn processar(&mut self, entradas: &[&[f32]], saidas: &mut [&mut [f32]], aj: &Ajustes) -> Result<Medidores> {
        let mut med = Medidores::default();
        for e in entradas {
            med.entrada = e.iter().fold(med.entrada, |m, &v| m.max(v.abs()));
        }
        let n_ch = entradas.len().min(saidas.len()).min(self.nucleo.canais.len());
        match &mut self.conversao {
            None => med.reducao_db = self.nucleo.processar(entradas, saidas, aj)?,
            Some(c) => {
                let n = entradas.first().map_or(0, |e| e.len());
                for ch in 0..n_ch {
                    c.fila_in[ch].extend(entradas[ch].iter().copied());
                }
                while c.fila_in[0].len() >= c.cin {
                    for ch in 0..n_ch {
                        for (d, s) in c.b_in[ch].iter_mut().zip(c.fila_in[ch].drain(..c.cin)) {
                            *d = s;
                        }
                    }
                    c.sobe.process_into_buffer(&c.b_in[..n_ch], &mut c.b48[..n_ch], None)
                        .map_err(|e| anyhow!("conversão de taxa: {e}"))?;
                    let ents: Vec<&[f32]> = c.b48[..n_ch].iter().map(|v| v.as_slice()).collect();
                    let mut sais: Vec<&mut [f32]> = c.b48_saida[..n_ch].iter_mut().map(|v| v.as_mut_slice()).collect();
                    let r = self.nucleo.processar(&ents, &mut sais, aj)?;
                    med.reducao_db = med.reducao_db.min(r);
                    c.desce.process_into_buffer(&c.b48_saida[..n_ch], &mut c.b_out[..n_ch], None)
                        .map_err(|e| anyhow!("conversão de taxa: {e}"))?;
                    for ch in 0..n_ch {
                        c.fila_out[ch].extend(c.b_out[ch].iter().copied());
                    }
                }
                for ch in 0..n_ch {
                    for (d, s) in saidas[ch].iter_mut().zip(c.fila_out[ch].drain(..n)) {
                        *d = s;
                    }
                }
                for s in saidas.iter_mut().skip(n_ch) {
                    s.fill(0.0);
                }
            }
        }
        // proteção: quando o volume é aumentado, a saída nunca passa de 0 dBFS. Sem aumento, o som
        // fica idêntico ao do app (que nos picos acima de 0 dBFS simplesmente corta).
        if aj.aumenta_volume() {
            self.limitador.processar(&mut saidas[..n_ch]);
        } else {
            self.limitador.ganho = 1.0;
        }
        for s in saidas[..n_ch].iter() {
            med.saida = s.iter().fold(med.saida, |m, &v| m.max(v.abs()));
        }
        Ok(med)
    }
}
