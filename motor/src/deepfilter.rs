//! DeepFilterNet (libDF v0.5.6, modelo DeepFilterNet3 embutido), reproduzindo
//! `deep-filter -D -a <nível>` e a leitura da saída int16 do protocolo.

use anyhow::{anyhow, Result};
use df::tract::{DfParams, DfTract, ReduceMask, RuntimeParams};
use ndarray::{Array2, ArrayView2, Axis};

use crate::config::Avancado;

/// Taxa de amostragem do modelo.
pub const SR_MODELO: u32 = 48_000;

/// Parâmetros de execução: com `Avancado::default()`, idênticos aos padrões do binário
/// `deep-filter` v0.5.6, com `-a <atten_lim_db>`.
fn parametros(atten_lim_db: f32, av: &Avancado) -> RuntimeParams {
    let p = RuntimeParams::default()
        .with_atten_lim(atten_lim_db)
        .with_thresholds(av.limiar_ruido_db, 35., 35.) // --min-db-thresh, --max-db-erb-thresh, --max-db-df-thresh
        .with_mask_reduce(ReduceMask::MAX); // --reduce-mask 1
    if av.pos_filtro > 0.0 {
        p.with_post_filter(av.pos_filtro) // --pf --pf-beta
    } else {
        p
    }
}

/// Executa o DeepFilterNet num sinal mono a 48 kHz já escalado como o binário o lê
/// (int16 / 32767). Retorna a saída compensada pelo atraso (`-D`), com `delay` amostras a menos,
/// em float, antes da conversão para int16.
///
/// `progresso` recebe a fração concluída (0..1).
pub fn executar(entrada: &[f32], atten_lim_db: f32, av: &Avancado, progresso: &dyn Fn(f32)) -> Result<Vec<f32>> {
    let mut modelo = DfTract::new(DfParams::default(), &parametros(atten_lim_db, av))
        .map_err(|e| anyhow!("falha ao carregar o modelo DeepFilterNet: {e}"))?;
    let hop = modelo.hop_size;
    let atraso = modelo.fft_size - modelo.hop_size + modelo.lookahead * modelo.hop_size;

    let ruido = ArrayView2::from_shape((1, entrada.len()), entrada)?;
    let mut enh = Array2::<f32>::zeros((1, entrada.len()));
    let total = entrada.len().max(1);
    let mut feito = 0;
    for (ns_f, enh_f) in ruido
        .axis_chunks_iter(Axis(1), hop)
        .zip(enh.axis_chunks_iter_mut(Axis(1), hop))
    {
        // Como no binário: o último bloco incompleto não é processado (fica em zero).
        if ns_f.len_of(Axis(1)) < hop {
            break;
        }
        modelo.process(ns_f, enh_f).map_err(|e| anyhow!("erro no DeepFilterNet: {e}"))?;
        feito += hop;
        if feito % (hop * 100) == 0 {
            progresso(feito as f32 / total as f32);
        }
    }
    progresso(1.0);
    let mut saida = enh.into_raw_vec();
    saida.drain(..atraso.min(saida.len()));
    Ok(saida)
}

/// Converte a entrada normalizada (x = int / 2^(bits-1)) na escala com que o binário lê o WAV.
/// Para int16 o binário divide por 32767 (não 32768).
pub fn escala_de_entrada(x: &[f32], int16: bool) -> Vec<f32> {
    if int16 {
        x.iter().map(|&v| (v * 32768.0) / 32767.0).collect()
    } else {
        x.to_vec()
    }
}

/// Etapa int16 do protocolo: o binário grava `(s * 32767) as i16` (truncado, saturado);
/// o protocolo lê esse int16, divide por 32768 e completa com zeros no FIM até `n` amostras.
pub fn int16_e_completar(saida: &[f32], n: usize) -> Vec<f32> {
    let mut y: Vec<f32> = saida
        .iter()
        .take(n)
        .map(|&s| (s * i16::MAX as f32) as i16 as f32 / 32768.0)
        .collect();
    y.resize(n, 0.0);
    y
}
