//! Ponte entre o motor do Decanta e o plugin AAX (AudioSuite).
//!
//! O plugin lê o clipe inteiro do Avid (um canal por vez, em float) e chama
//! `decanta_processar`. O caminho é o mesmo do app para uma versão só, no modo
//! "um mono por canal" e na taxa original: 48 kHz → DeepFilterNet → etapa int16 do
//! protocolo → residual (se pedido) → volta à taxa do projeto.

pub mod compressor;
pub mod tempo_real;

use std::ffi::c_void;

use anyhow::{anyhow, bail, Result};
use ndarray::ArrayView2;
use tratamento_motor::{deepfilter, para_48k, Avancado};

/// Ajustes do tratamento (os mesmos da tela do app).
#[derive(Debug, Clone, Copy)]
pub struct Ajustes {
    /// Intensidade: limite de atenuação do DeepFilterNet (0 a 100 dB).
    pub atenuacao_db: f32,
    pub avancado: Avancado,
    /// Devolve o que foi retirado (original − tratado) em vez do tratado.
    pub retirado: bool,
}

/// O áudio veio de um arquivo 16-bit? (todas as amostras são múltiplos exatos de 1/32768)
///
/// O app sabe isso pelo cabeçalho do WAV; no Avid só temos o float, então conferimos as amostras.
/// Silêncio digital puro também passa no teste, o que não muda o resultado.
pub fn parece_16bit(x: &[f32]) -> bool {
    x.iter().all(|&v| {
        let s = v * 32768.0;
        s == s.trunc() && (-32768.0..=32767.0).contains(&s)
    })
}

fn reamostrar(x: &[f32], de: u32, para: u32) -> Result<Vec<f32>> {
    if de == para {
        return Ok(x.to_vec());
    }
    let v = ArrayView2::from_shape((1, x.len()), x)?;
    Ok(df::transforms::resample(v, de as usize, para as usize, None)
        .map_err(|e| anyhow!("falha na reamostragem: {e}"))?
        .into_raw_vec())
}

/// Trata um canal mono na taxa `sr`. Devolve o mesmo número de amostras, na mesma taxa.
pub fn processar_canal(x: &[f32], sr: u32, aj: &Ajustes, progresso: &dyn Fn(f32)) -> Result<Vec<f32>> {
    if x.is_empty() {
        return Ok(Vec::new());
    }
    if !(8_000..=192_000).contains(&sr) {
        bail!("taxa de amostragem não suportada: {sr} Hz");
    }
    let int16 = parece_16bit(x);
    let original = para_48k(x, sr, int16)?;
    let n = original.len();
    let entrada = deepfilter::escala_de_entrada(&original, int16);
    let y = deepfilter::executar(&entrada, aj.atenuacao_db, &aj.avancado, progresso)?;
    let mut y = deepfilter::int16_e_completar(&y, n);
    if aj.retirado {
        for (r, &o) in y.iter_mut().zip(&original) {
            *r = (o as f64 - *r as f64) as f32;
        }
    }
    let sr48 = deepfilter::SR_MODELO;
    let mut saida = reamostrar(&y, sr48, sr)?;
    saida.resize(x.len(), 0.0);
    Ok(saida)
}

// ---------------------------------------------------------------------------------------------
// Interface em C (usada pelo plugin)

/// Recebe a fração concluída (0..1) e o `contexto` passado pelo plugin.
pub type FuncaoProgresso = Option<extern "C" fn(fracao: f32, contexto: *mut c_void)>;

/// Códigos de retorno.
pub const DECANTA_OK: i32 = 0;
pub const DECANTA_ERRO_PARAMETRO: i32 = -1;
pub const DECANTA_ERRO_PROCESSAMENTO: i32 = -2;

/// Trata `n` amostras de `entrada` (um canal, taxa `sr`) e grava `n` amostras em `saida`.
///
/// `retirado` ≠ 0 devolve original − tratado. `pos_filtro` (0 a 0,2) e `limiar_ruido_db`
/// (−40 a 10) são os ajustes avançados do app; use 0 e −15 para o padrão.
///
/// # Safety
/// `entrada` e `saida` devem apontar para `n` floats válidos (podem ser o mesmo buffer).
#[no_mangle]
pub unsafe extern "C" fn decanta_processar(
    entrada: *const f32,
    saida: *mut f32,
    n: usize,
    sr: u32,
    atenuacao_db: f32,
    pos_filtro: f32,
    limiar_ruido_db: f32,
    retirado: i32,
    progresso: FuncaoProgresso,
    contexto: *mut c_void,
) -> i32 {
    if entrada.is_null() || saida.is_null() {
        return DECANTA_ERRO_PARAMETRO;
    }
    if !(0.0..=100.0).contains(&atenuacao_db)
        || !(0.0..=0.2).contains(&pos_filtro)
        || !(-40.0..=10.0).contains(&limiar_ruido_db)
    {
        return DECANTA_ERRO_PARAMETRO;
    }
    let x = std::slice::from_raw_parts(entrada, n).to_vec();
    let aj = Ajustes {
        atenuacao_db,
        avancado: Avancado { pos_filtro, limiar_ruido_db },
        retirado: retirado != 0,
    };
    let ctx = contexto as usize; // o callback é chamado na mesma thread
    let cb = move |f: f32| {
        if let Some(p) = progresso {
            p(f, ctx as *mut c_void)
        }
    };
    match processar_canal(&x, sr, &aj, &cb) {
        Ok(y) => {
            std::ptr::copy_nonoverlapping(y.as_ptr(), saida, n);
            DECANTA_OK
        }
        Err(_) => DECANTA_ERRO_PROCESSAMENTO,
    }
}

// ---------------------------------------------------------------------------------------------
// Tempo real (VST3/AU/AAX de faixa)

/// Ajustes do modo tempo real, como o plugin os passa (ver `tempo_real::Ajustes`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DecantaAjustesRT {
    pub separacao_db: f32,
    pub pos_filtro: f32,
    pub limiar_ruido_db: f32,
    pub voz_db: f32,
    pub ambiencia_db: f32,
    pub ganho_db: f32,
    /// Botão Compressão, 0 a 1.
    pub compressao: f32,
    pub comp_manual: i32,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub so_fundo: i32,
}

/// Arredonda ao passo do botão: o programa guarda os parâmetros de 0 a 1, e "0 dB" volta como
/// 0,0000004 dB, o que tiraria o padrão do som exato do app.
fn passo(v: f32, p: f32) -> f32 {
    (v / p).round() * p
}

impl DecantaAjustesRT {
    fn ajustes(&self) -> tempo_real::Ajustes {
        tempo_real::Ajustes {
            separacao_db: passo(self.separacao_db, 1.0).clamp(0.0, 100.0),
            pos_filtro: self.pos_filtro.clamp(0.0, 0.2),
            limiar_ruido_db: self.limiar_ruido_db.clamp(-40.0, 10.0),
            voz_db: passo(self.voz_db, 0.1),
            ambiencia_db: passo(self.ambiencia_db, 0.1),
            compressor: compressor::ParamsCompressor {
                quantidade: self.compressao,
                manual: self.comp_manual != 0,
                threshold_db: self.threshold_db,
                ratio: self.ratio,
                attack_ms: self.attack_ms,
                release_ms: self.release_ms,
                knee_db: self.knee_db,
            },
            ganho_db: passo(self.ganho_db, 0.1),
            so_fundo: self.so_fundo != 0,
        }
    }
}

/// Um Decanta em tempo real com a cópia da entrada (o host costuma usar o mesmo buffer).
pub struct DecantaRT {
    rt: tempo_real::TempoReal,
    copia: Vec<Vec<f32>>,
}

/// Cria o Decanta em tempo real. Carrega o modelo: chamar fora da thread de áudio.
/// Devolve nulo em caso de erro.
#[no_mangle]
pub extern "C" fn decanta_rt_novo(n_canais: u32, sr: u32) -> *mut DecantaRT {
    match tempo_real::TempoReal::novo(n_canais as usize, sr, &tempo_real::Ajustes::default()) {
        Ok(rt) => Box::into_raw(Box::new(DecantaRT { rt, copia: vec![Vec::new(); n_canais.max(1) as usize] })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `h` veio de `decanta_rt_novo` e não é usado depois.
#[no_mangle]
pub unsafe extern "C" fn decanta_rt_liberar(h: *mut DecantaRT) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// Atraso da saída, em amostras (0 se a taxa não for 48 kHz).
///
/// # Safety
/// `h` válido.
#[no_mangle]
pub unsafe extern "C" fn decanta_rt_latencia(h: *const DecantaRT) -> u32 {
    h.as_ref().map_or(0, |d| d.rt.latencia() as u32)
}

/// 1 se trata o áudio (48 kHz), 0 se só deixa passar.
///
/// # Safety
/// `h` válido.
#[no_mangle]
pub unsafe extern "C" fn decanta_rt_ativo(h: *const DecantaRT) -> i32 {
    h.as_ref().map_or(0, |d| d.rt.ativo() as i32)
}

/// Volta ao estado inicial. Recarrega o modelo: chamar fora da thread de áudio.
///
/// # Safety
/// `h` válido.
#[no_mangle]
pub unsafe extern "C" fn decanta_rt_reiniciar(h: *mut DecantaRT) -> i32 {
    match h.as_mut() {
        Some(d) => match d.rt.reiniciar() {
            Ok(()) => DECANTA_OK,
            Err(_) => DECANTA_ERRO_PROCESSAMENTO,
        },
        None => DECANTA_ERRO_PARAMETRO,
    }
}

/// Trata um bloco de `n` amostras em `n_canais` canais. Entrada e saída podem ser o mesmo buffer.
///
/// # Safety
/// `h` válido; `entradas`/`saidas` com `n_canais` ponteiros para `n` floats; `aj` válido;
/// `medidores` nulo ou válido.
#[no_mangle]
pub unsafe extern "C" fn decanta_rt_processar(
    h: *mut DecantaRT,
    entradas: *const *const f32,
    saidas: *const *mut f32,
    n_canais: u32,
    n: u32,
    aj: *const DecantaAjustesRT,
    medidores: *mut tempo_real::Medidores,
) -> i32 {
    let (Some(d), Some(aj)) = (h.as_mut(), aj.as_ref()) else {
        return DECANTA_ERRO_PARAMETRO;
    };
    if entradas.is_null() || saidas.is_null() {
        return DECANTA_ERRO_PARAMETRO;
    }
    let (nc, n) = (n_canais as usize, n as usize);
    if d.copia.len() < nc {
        d.copia.resize(nc, Vec::new());
    }
    for c in 0..nc {
        let e = *entradas.add(c);
        d.copia[c].clear();
        if e.is_null() {
            d.copia[c].resize(n, 0.0);
        } else {
            d.copia[c].extend_from_slice(std::slice::from_raw_parts(e, n));
        }
    }
    let ents: Vec<&[f32]> = d.copia[..nc].iter().map(|v| v.as_slice()).collect();
    let mut sais: Vec<&mut [f32]> = Vec::with_capacity(nc);
    for c in 0..nc {
        let s = *saidas.add(c);
        if s.is_null() {
            return DECANTA_ERRO_PARAMETRO;
        }
        sais.push(std::slice::from_raw_parts_mut(s, n));
    }
    match d.rt.processar(&ents, &mut sais, &aj.ajustes()) {
        Ok(m) => {
            if let Some(md) = medidores.as_mut() {
                *md = m;
            }
            DECANTA_OK
        }
        Err(_) => DECANTA_ERRO_PROCESSAMENTO,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn detecta_16bit() {
        assert!(parece_16bit(&[0.0, 0.5, -1.0, 12345.0 / 32768.0]));
        assert!(!parece_16bit(&[0.0, 1.0 / 8_388_608.0]));
    }
}
