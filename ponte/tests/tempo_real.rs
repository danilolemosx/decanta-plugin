//! Modo tempo real: com os ajustes padrão a voz é a mesma do DeepFilterNet do app (antes da
//! etapa int16), atrasada de LATENCIA, qualquer que seja o tamanho dos blocos do programa.

use std::path::PathBuf;

use decanta_ponte::tempo_real::{Ajustes, TempoReal, DB_MUDO, LATENCIA};
use tratamento_motor::{deepfilter, wav, Avancado};

fn sinal_de_teste(segundos: usize) -> Vec<f32> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-audio/gravacao-teste-decanta.wav");
    let w = wav::ler(&p).unwrap();
    assert_eq!(w.sr, 48_000);
    w.canais[0][..(segundos * 48_000).min(w.canais[0].len())].to_vec()
}

/// Passa `x` pelo tempo real em blocos de tamanhos variados (como um programa faria).
fn rodar(rt: &mut TempoReal, x: &[f32], aj: &Ajustes, blocos: &[usize]) -> Vec<f32> {
    let mut y = vec![0.0; x.len()];
    let (mut pos, mut k) = (0, 0);
    while pos < x.len() {
        let n = blocos[k % blocos.len()].min(x.len() - pos);
        let mut s = vec![0.0; n];
        {
            let mut saidas = [&mut s[..]];
            rt.processar(&[&x[pos..pos + n]], &mut saidas, aj).unwrap();
        }
        y[pos..pos + n].copy_from_slice(&s);
        pos += n;
        k += 1;
    }
    y
}

#[test]
fn voz_identica_ao_motor_em_qualquer_bloco() {
    // o arquivo inteiro: tem picos em 0 dBFS, onde o limitador não pode mexer no padrão
    let x = sinal_de_teste(55);
    let aj = Ajustes::default();
    let referencia = deepfilter::executar(&x, aj.separacao_db, &Avancado::default(), &|_| {}).unwrap();
    for blocos in [vec![512], vec![480], vec![64, 1000, 333, 4096, 7]] {
        let mut rt = TempoReal::novo(1, 48_000, &aj).unwrap();
        let y = rodar(&mut rt, &x, &aj, &blocos);
        // antes da latência sai o "aquecimento" do modelo, que o app descarta
        assert!(y[..480].iter().all(|&v| v == 0.0));
        let n = referencia.len().min(y.len() - LATENCIA);
        let diferentes = (0..n).filter(|&i| y[i + LATENCIA] != referencia[i]).count();
        assert_eq!(diferentes, 0, "blocos {blocos:?}");
    }
}

#[test]
fn voz_mais_fundo_devolve_o_original() {
    let x = sinal_de_teste(4);
    let aj = Ajustes { separacao_db: 100.0, ambiencia_db: 0.0, ..Default::default() };
    let mut rt = TempoReal::novo(1, 48_000, &aj).unwrap();
    let y = rodar(&mut rt, &x, &aj, &[256]);
    let pior = (0..x.len() - LATENCIA).map(|i| (y[i + LATENCIA] - x[i]).abs()).fold(0f32, f32::max);
    assert!(pior < 1e-6, "diferença máxima {pior}");
}

#[test]
fn so_fundo_e_original_menos_voz() {
    let x = sinal_de_teste(4);
    let base = Ajustes { separacao_db: 100.0, ..Default::default() };
    let voz = rodar(&mut TempoReal::novo(1, 48_000, &base).unwrap(), &x, &base, &[512]);
    let aj = Ajustes { so_fundo: true, ..base };
    let fundo = rodar(&mut TempoReal::novo(1, 48_000, &aj).unwrap(), &x, &aj, &[512]);
    for i in 0..x.len() - LATENCIA {
        let esperado = (x[i] as f64 - voz[i + LATENCIA] as f64) as f32;
        assert_eq!(fundo[i + LATENCIA], esperado);
    }
}

/// Sinal sintético na taxa `sr`: tons de 100 Hz a 15 kHz (a faixa da fala com folga; só os que a
/// taxa comporta), com volume variando, como uma fala.
fn sintetico(sr: u32, segundos: f32) -> Vec<f32> {
    let n = (sr as f32 * segundos) as usize;
    let tons = [100.0f32, 220.0, 470.0, 1000.0, 2300.0, 4700.0, 8000.0, 11000.0, 15000.0];
    (0..n)
        .map(|i| {
            let t = i as f32 / sr as f32;
            let envelope = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 2.0 * t).sin();
            envelope * tons.iter().enumerate().filter(|(_, &f)| f < 0.4 * sr as f32).map(|(k, f)| (0.05 / (1.0 + k as f32 * 0.2)) * (2.0 * std::f32::consts::PI * f * t + k as f32).sin()).sum::<f32>()
        })
        .collect()
}

#[test]
fn outras_taxas_convertem_com_atraso_exato() {
    for sr in [44_100u32, 88_200, 96_000, 32_000] {
        let x = sintetico(sr, 3.0);
        let aj = Ajustes { separacao_db: 100.0, ambiencia_db: 0.0, ..Default::default() };
        let mut rt = TempoReal::novo(1, sr, &aj).unwrap();
        let lat = rt.latencia();
        let y = rodar(&mut rt, &x, &aj, &[512, 300, 1024]);
        // melhor alinhamento perto do atraso informado
        let inicio = sr as usize / 2;
        let n = x.len() - inicio - lat - 100;
        let erro = |d: usize| -> f64 {
            (inicio..inicio + n).map(|i| ((y[i + d] - x[i]) as f64).powi(2)).sum::<f64>()
        };
        let melhor = (lat - 20..=lat + 20).min_by(|&a, &b| erro(a).partial_cmp(&erro(b)).unwrap()).unwrap();
        let energia: f64 = (inicio..inicio + n).map(|i| (x[i] as f64).powi(2)).sum();
        let snr = 10.0 * (energia / erro(lat)).log10();
        eprintln!("{sr} Hz: atraso informado {lat}, medido {melhor}, fidelidade {snr:.1} dB");
        assert_eq!(melhor, lat, "{sr} Hz");
        assert!(snr > 40.0, "{sr} Hz: ida e volta pouco fiel ({snr:.1} dB)");
    }
}

#[test]
fn ajustes_mudando_enquanto_toca() {
    let x = sinal_de_teste(4);
    let mut rt = TempoReal::novo(2, 48_000, &Ajustes::default()).unwrap();
    let mut y = vec![vec![0.0f32; 512]; 2];
    for (k, bloco) in x.chunks(512).enumerate() {
        let aj = Ajustes {
            separacao_db: (k % 100) as f32,
            pos_filtro: if k % 7 == 0 { 0.05 } else { 0.0 },
            voz_db: -((k % 13) as f32),
            ambiencia_db: if k % 2 == 0 { DB_MUDO } else { -12.0 },
            ganho_db: 6.0,
            compressor: decanta_ponte::compressor::ParamsCompressor { quantidade: 0.6, ..Default::default() },
            ..Default::default()
        };
        let n = bloco.len();
        let (a, b) = y.split_at_mut(1);
        let mut saidas = [&mut a[0][..n], &mut b[0][..n]];
        let m = rt.processar(&[bloco, bloco], &mut saidas, &aj).unwrap();
        assert!(m.saida.is_finite() && m.reducao_db <= 0.0);
        assert!(saidas.iter().all(|s| s.iter().all(|v| v.is_finite())));
    }
    rt.reiniciar().unwrap();
}

#[test]
fn limitador_segura_0_dbfs() {
    let x = sinal_de_teste(4);
    let aj = Ajustes {
        separacao_db: 100.0,
        ganho_db: 12.0,
        compressor: decanta_ponte::compressor::ParamsCompressor { quantidade: 0.3, ..Default::default() },
        ..Default::default()
    };
    let mut rt = TempoReal::novo(1, 48_000, &aj).unwrap();
    let y = rodar(&mut rt, &x, &aj, &[512]);
    let pico = y.iter().fold(0f32, |m, &v| m.max(v.abs()));
    assert!(pico <= 1.0, "pico {pico}");
    assert!(pico > 0.9, "o limitador não deveria abaixar demais: {pico}");
}
