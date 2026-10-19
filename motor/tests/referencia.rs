//! Compara o motor com o binário de referência deep-filter v0.5.6.
//!
//! Requer a variável DEEP_FILTER_BIN com o caminho do binário
//! (deep-filter-0.5.6-x86_64-unknown-linux-musl). Sem ela, o teste é pulado.
//! WAV de teste: TESTE_WAV ou test-audio/gravacao-teste-decanta.wav.

use std::path::PathBuf;
use std::process::Command;

use tratamento_motor::{deepfilter, processar, wav, Configuracao, NIVEIS};

fn wav_de_teste() -> PathBuf {
    std::env::var_os("TESTE_WAV").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-audio/gravacao-teste-decanta.wav")
    })
}

#[test]
fn identico_ao_binario_de_referencia() {
    let Some(bin) = std::env::var_os("DEEP_FILTER_BIN") else {
        eprintln!("DEEP_FILTER_BIN não definido: teste de referência pulado");
        return;
    };
    let entrada = wav_de_teste();
    let original = wav::ler(&entrada).expect("WAV de teste");
    assert_eq!(original.sr, 48_000);
    let tmp = std::env::temp_dir().join(format!("motor-ref-{}", std::process::id()));

    // Pipeline completo do app
    let cfg = Configuracao { pasta_saida: Some(tmp.join("app")), ..Default::default() };
    let rel = processar(&entrada, &cfg, &|_| {}).unwrap();
    let n = original.n_amostras();

    for (canal, res) in rel.canais.iter().enumerate() {
        assert_eq!(res.atraso, 0);
        for f in &res.arquivos {
            let w = wav::ler(&f.caminho).unwrap();
            assert_eq!(w.n_amostras(), n, "{}", f.caminho.display());
            assert_eq!(w.time_reference, original.time_reference, "{}", f.caminho.display());
            assert_eq!(f.time_reference, original.time_reference);
        }
        for &(nivel, rotulo) in NIVEIS.iter() {
            let app = res.sinais.versao(rotulo).unwrap();
            let out = tmp.join(format!("df_{nivel}"));
            let st = Command::new(&bin)
                .args(["-D", "-a", &nivel.to_string(), "-o"])
                .arg(&out)
                .arg(&entrada)
                .status()
                .expect("executar deep-filter");
            assert!(st.success());
            let refw = wav::ler(&out.join(entrada.file_name().unwrap())).unwrap();
            let r = &refw.canais[canal];
            let atraso = n - r.len();
            let mut dif_max = 0f32;
            let mut n_dif = 0;
            for (a, b) in r.iter().zip(app.iter()) {
                if a != b {
                    n_dif += 1;
                    dif_max = dif_max.max((a - b).abs());
                }
            }
            let zeros_no_fim = app[r.len()..].iter().all(|&v| v == 0.0);
            println!(
                "{rotulo:>6} (-a {nivel}): {} amostras do binário (+{atraso} zeros no fim), {n_dif} diferentes, dif. máx {} LSB 16-bit",
                r.len(),
                dif_max * 32768.0
            );
            assert_eq!(atraso, 1440);
            assert!(zeros_no_fim);
            assert_eq!(n_dif, 0, "{rotulo}: saída diferente do binário de referência");
        }
        let residual_ok = res
            .sinais
            .original
            .iter()
            .zip(res.sinais.versao("forte").unwrap())
            .zip(res.sinais.residual.as_ref().unwrap())
            .all(|((&o, &f), &r)| r == (o as f64 - f as f64) as f32);
        assert!(residual_ok);
    }
    // sanidade da escala de entrada int16 do binário
    assert_eq!(deepfilter::escala_de_entrada(&[-1.0, 0.5], true), vec![-32768.0 / 32767.0, 16384.0 / 32767.0]);
    std::fs::remove_dir_all(tmp).ok();
}
