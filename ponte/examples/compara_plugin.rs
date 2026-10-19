//! Compara a saída do plugin compilado (gravada pelo testa-plugin) com a referência da ponte.
//!
//!   cargo run --release --example compara_plugin -- <entrada.wav> <saida_plugin.wav> <modo> [separacao]
//!
//! modos: "voz" (padrão do plugin: tem de ser idêntica ao DeepFilterNet, atrasada),
//!        "original" (voz + ambiência em 0 dB: volta o original), "passa" (sem tratamento).

use std::path::Path;

use decanta_ponte::tempo_real::LATENCIA;
use tratamento_motor::{deepfilter, wav, Avancado};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let entrada = wav::ler(Path::new(&a[1])).expect("entrada");
    let plugin = wav::ler(Path::new(&a[2])).expect("saída do plugin");
    let modo = a[3].as_str();
    let separacao: f32 = a.get(4).map_or(20.0, |s| s.parse().unwrap());
    assert_eq!(entrada.canais.len(), plugin.canais.len(), "canais");
    let mut pior = 0f32;
    let mut diferentes = 0usize;
    let mut total = 0usize;
    for (x, y) in entrada.canais.iter().zip(&plugin.canais) {
        let referencia: Vec<f32> = match modo {
            "voz" => deepfilter::executar(x, separacao, &Avancado::default(), &|_| {}).unwrap(),
            "original" => x.clone(),
            "passa" => x.clone(),
            _ => panic!("modo desconhecido"),
        };
        let atraso = if modo == "passa" { 0 } else { LATENCIA };
        let n = referencia.len().min(y.len() - atraso);
        for i in 0..n {
            let d = (y[i + atraso] - referencia[i]).abs();
            pior = pior.max(d);
            diferentes += (d != 0.0) as usize;
        }
        total += n;
    }
    println!("{modo}: {total} amostras comparadas, {diferentes} diferentes, diferença máxima {pior:e}");
}
