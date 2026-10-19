//! O plugin tem de gerar exatamente o mesmo áudio que o app Decanta.
//!
//! Para cada caso: o app (`motor::processar`, uma versão, um mono por canal, WAV 24-bit) grava
//! o tratado e o retirado; a ponte trata os mesmos canais em float, como o Avid entrega, e o
//! resultado é gravado em 24-bit pelo mesmo gravador. As amostras têm de ser idênticas.
//!
//! Áudios: test-audio/gravacao-teste-decanta.wav (16-bit, 48 kHz, mono, 19 s) e, se houver ffmpeg, uma
//! cópia 24-bit, 44,1 kHz, estéreo (exercita a reamostragem e a entrada não 16-bit).

use std::path::{Path, PathBuf};
use std::process::Command;

use decanta_ponte::{processar_canal, Ajustes};
use tratamento_motor::{processar, wav, Avancado, Configuracao, ModoCanais, Residual, Versao};

fn wav_de_teste() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-audio/gravacao-teste-decanta.wav")
}

fn pasta_temp(nome: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("decanta-ponte-{}-{nome}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Cópia 24-bit, 44,1 kHz, estéreo (canal 2 = canal 1 com ganho e atraso), via ffmpeg.
fn wav_24bit_estereo(pasta: &Path) -> Option<PathBuf> {
    let destino = pasta.join("estereo_24bit_44k.wav");
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(wav_de_teste())
        .args([
            "-filter_complex",
            "[0:a]asplit[a][b];[b]volume=0.7,adelay=37[c];[a][c]amerge=inputs=2",
            "-ar",
            "44100",
            "-c:a",
            "pcm_s24le",
            "-t",
            "20",
        ])
        .arg(&destino)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok.then_some(destino)
}

fn comparar(entrada: &Path, atenuacao_db: f32, avancado: Avancado, pasta: &Path) {
    let cfg = Configuracao {
        versoes: vec![Versao { id: "df".into(), ativo: true, atenuacao_db, sufixo: "DF".into() }],
        residual: Residual { ativo: true, base: "df".into(), sufixo: "retirado".into() },
        canais: ModoCanais::MonoPorCanal,
        pasta_saida: Some(pasta.join("app")),
        avancado,
        ..Default::default()
    };
    let rel = processar(entrada, &cfg, &|_| {}).unwrap();
    let original = wav::ler(entrada).unwrap();

    for (c, res) in rel.canais.iter().enumerate() {
        for arq in &res.arquivos {
            let retirado = arq.rotulo == "residual";
            let aj = Ajustes { atenuacao_db, avancado, retirado };
            let y = processar_canal(&original.canais[c], original.sr, &aj, &|_| {}).unwrap();
            let nosso = pasta.join(format!("ponte_c{c}_{}.wav", arq.rotulo));
            wav::gravar_bext(&nosso, &[&y], 24, original.sr, 0, "").unwrap();

            let app = wav::ler(&arq.caminho).unwrap();
            let ponte = wav::ler(&nosso).unwrap();
            assert_eq!(app.sr, ponte.sr);
            assert_eq!(app.canais[0].len(), ponte.canais[0].len(), "{}", arq.caminho.display());
            let diferentes = app.canais[0].iter().zip(&ponte.canais[0]).filter(|(a, b)| a != b).count();
            assert_eq!(
                diferentes,
                0,
                "{}: {diferentes} amostras diferentes ({} dB, {:?})",
                arq.caminho.display(),
                atenuacao_db,
                avancado
            );
            eprintln!(
                "idêntico: {} canal {} {} ({} dB) — {} amostras",
                entrada.file_name().unwrap().to_string_lossy(),
                c + 1,
                arq.rotulo,
                atenuacao_db,
                app.canais[0].len()
            );
        }
    }
}

#[test]
fn identico_ao_app_16bit_48k_mono() {
    let pasta = pasta_temp("16bit");
    for db in [12.0, 20.0, 100.0] {
        comparar(&wav_de_teste(), db, Avancado::default(), &pasta);
    }
    comparar(&wav_de_teste(), 35.0, Avancado { pos_filtro: 0.02, limiar_ruido_db: -10.0 }, &pasta);
    std::fs::remove_dir_all(&pasta).ok();
}

#[test]
fn identico_ao_app_24bit_44k_estereo() {
    let pasta = pasta_temp("24bit");
    let Some(entrada) = wav_24bit_estereo(&pasta) else {
        eprintln!("ffmpeg não encontrado: caso 24-bit pulado");
        return;
    };
    let w = wav::ler(&entrada).unwrap();
    assert!(!decanta_ponte::parece_16bit(&w.canais[0]));
    comparar(&entrada, 20.0, Avancado::default(), &pasta);
    std::fs::remove_dir_all(&pasta).ok();
}
