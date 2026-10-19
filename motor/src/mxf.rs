//! Exportação em MXF OP-Atom para o Avid, via `raw2bmx -t avid` (bmx, EBU), como no fluxo de SFX.
//! O áudio é gravado num WAV 24-bit temporário e embrulhado sem alteração (48 kHz).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::timecode::{timecode, Fps};
use crate::wav;

/// Grava `canais` (48 kHz) como um clipe MXF do Avid: `<prefixo>_a1.mxf`, `_a2.mxf`...
/// Devolve os arquivos criados.
#[allow(clippy::too_many_arguments)]
pub fn gravar(
    raw2bmx: &Path,
    prefixo: &Path,
    canais: &[&[f32]],
    sr: u32,
    time_reference: u64,
    fps: Fps,
    clipe: &str,
    fita: &str,
) -> Result<Vec<PathBuf>> {
    if sr != 48_000 {
        bail!("MXF para o Avid exige 48 kHz");
    }
    let nome = prefixo.file_name().context("nome de saída inválido")?.to_string_lossy().into_owned();
    let pasta = prefixo.parent().context("pasta de saída inválida")?;
    let esperados: Vec<PathBuf> = (1..=canais.len()).map(|k| pasta.join(format!("{nome}_a{k}.mxf"))).collect();
    for f in &esperados {
        let _ = std::fs::remove_file(f); // regravar por cima, como nos outros formatos
    }
    let temp = std::env::temp_dir().join(format!("decanta-{}-{}.wav", std::process::id(), nome.len()));
    wav::gravar_bext(&temp, canais, 24, sr, time_reference, clipe)?;
    let (num, den) = fps.fracao();
    let mut cmd = Command::new(raw2bmx);
    cmd.args(["-t", "avid", "-f", &format!("{num}/{den}"), "-y", &timecode(time_reference, sr, fps)])
        .args(["--project", "Decanta", "--clip", clipe, "--tape", fita])
        .arg("-o")
        .arg(prefixo)
        .arg("--wave")
        .arg(&temp);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let saida = cmd.output();
    let _ = std::fs::remove_file(&temp);
    let saida = saida.with_context(|| format!("não foi possível executar {}", raw2bmx.display()))?;
    if !saida.status.success() {
        bail!("raw2bmx falhou: {}", String::from_utf8_lossy(&saida.stderr).trim());
    }
    for f in &esperados {
        let ok = std::fs::metadata(f).map(|m| m.len() > 0).unwrap_or(false);
        if !ok {
            bail!("raw2bmx não gerou {}", f.display());
        }
    }
    Ok(esperados)
}

/// Amostras que sobram além do quadro inteiro (o timecode do MXF começa num quadro).
pub fn sobra_do_quadro(time_reference: u64, sr: u32, fps: Fps) -> u64 {
    let (num, den) = fps.fracao();
    // amostras por quadro = sr * den / num (pode ser fracionário, ex. 2002 a 23,976)
    let resto = (time_reference as u128 * num as u128) % (sr as u128 * den as u128);
    (resto / num as u128) as u64
}

/// Número de amostras que fecha o último quadro (arredonda `n` para cima, em quadros inteiros).
pub fn completar_quadro(n: usize, sr: u32, fps: Fps) -> usize {
    let (num, den) = fps.fracao();
    let por_quadro = sr as u128 * den as u128; // amostras × num por quadro
    let quadros = (n as u128 * num as u128).div_ceil(por_quadro);
    (quadros * por_quadro).div_ceil(num as u128) as usize
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn quadros() {
        // arquivo de teste: 173106934 = 86467 × 2002 → alinhado a 23,976
        assert_eq!(sobra_do_quadro(173_106_934, 48_000, Fps::F23_976), 0);
        assert_eq!(sobra_do_quadro(173_106_934, 48_000, Fps::F25), 1654);
        assert_eq!(completar_quadro(2_656_654, 48_000, Fps::F23_976), 2_656_654);
        assert_eq!(completar_quadro(2_658_308, 48_000, Fps::F25), 2_659_200);
        assert_eq!(completar_quadro(1, 48_000, Fps::F29_97), 1602);
    }
}
