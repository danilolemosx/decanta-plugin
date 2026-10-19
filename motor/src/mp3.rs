//! Exportação em MP3 (LAME, 320 kbps CBR). MP3 não guarda timecode.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, MonoPcm, Quality};

/// Grava `canais` (1 ou 2, mesmo tamanho) como MP3 320 kbps.
pub fn gravar(caminho: &Path, canais: &[&[f32]], sr: u32) -> Result<()> {
    if canais.is_empty() || canais.len() > 2 {
        bail!("MP3 aceita só mono ou estéreo ({} canais pedidos)", canais.len());
    }
    let mut b = Builder::new().context("falha ao iniciar o codificador MP3")?;
    b.set_num_channels(canais.len() as u8).map_err(|e| anyhow!("MP3: {e}"))?;
    b.set_sample_rate(sr).map_err(|e| anyhow!("MP3: taxa {sr} Hz não suportada ({e})"))?;
    b.set_brate(Bitrate::Kbps320).map_err(|e| anyhow!("MP3: {e}"))?;
    b.set_quality(Quality::Best).map_err(|e| anyhow!("MP3: {e}"))?;
    let mut enc = b.build().map_err(|e| anyhow!("MP3: {e}"))?;

    let mut saida = Vec::new();
    // em blocos, para não alocar o pior caso do arquivo inteiro de uma vez
    const BLOCO: usize = 1 << 16;
    let n = canais[0].len();
    let mut i = 0;
    while i < n {
        let f = (i + BLOCO).min(n);
        saida.reserve(mp3lame_encoder::max_required_buffer_size(f - i));
        let r = if canais.len() == 1 {
            enc.encode_to_vec(MonoPcm(&canais[0][i..f]), &mut saida)
        } else {
            enc.encode_to_vec(DualPcm { left: &canais[0][i..f], right: &canais[1][i..f] }, &mut saida)
        };
        r.map_err(|e| anyhow!("MP3: {e}"))?;
        i = f;
    }
    saida.reserve(7200);
    enc.flush_to_vec::<FlushNoGap>(&mut saida).map_err(|e| anyhow!("MP3: {e}"))?;
    fs::write(caminho, &saida).with_context(|| format!("não foi possível criar {}", caminho.display()))?;
    Ok(())
}
