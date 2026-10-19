//! CLI de teste do motor.
//!
//! tratamento-cli [opções] arquivo.wav...  (sem opções = protocolo aprovado)

use std::path::PathBuf;
use std::process::ExitCode;

use tratamento_motor::{espectrograma, inspecionar, processar, Configuracao, FormatoSaida, Fps, ModoCanais, Residual, Versao};

const USO: &str = "uso: tratamento-cli [--inspecionar] [--fps FPS] [--saida PASTA] [--espectros PASTA] arquivo.wav...
  --inspecionar      só inspeciona (sample rate, canais, timecode, clip, silêncio)
  --fps FPS          frame rate para exibir o timecode: 23.976 (padrão), 24, 25, 29.97, 29.97df, 30
  --saida PASTA      grava as saídas nesta pasta (padrão: ao lado do original)
  --niveis A,B,C     intensidades leve,médio,forte em dB (padrão 12,20,100)
  --unica DB         como o app: uma versão só (<nome>_DF<DB>) + o retirado (<nome>_DF<DB>_retirado)
  --pf BETA          liga o pós-filtro com esta força (ex.: 0.02)
  --limiar DB        limiar de «só ruído» (padrão −15)
  --formato F        wav24 (padrão), wav16, mp3 ou mxf (Avid; usa --fps e --bmx)
  --bmx CAMINHO      raw2bmx do bmx, para --formato mxf
  --canais M         mono_por_canal (padrão), mono_soma, igual_original ou estereo
  --taxa HZ          taxa de saída (padrão: a do original)
  --espectros PASTA  grava PNGs dos espectrogramas original / forte / residual";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut so_inspecionar = false;
    let mut opcoes = Configuracao::default();
    let mut espectros: Option<PathBuf> = None;
    let mut fps = Fps::F23_976;
    let mut arquivos = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--inspecionar" => so_inspecionar = true,
            "--saida" => opcoes.pasta_saida = args.next().map(PathBuf::from),
            "--espectros" => espectros = args.next().map(PathBuf::from),
            "--bmx" => opcoes.raw2bmx = args.next().map(PathBuf::from),
            "--niveis" | "--unica" | "--pf" | "--limiar" | "--formato" | "--canais" | "--taxa" => {
                let v = args.next().unwrap_or_default();
                if let Err(e) = aplicar(&mut opcoes, &a, &v) {
                    eprintln!("{e}\n{USO}");
                    return ExitCode::from(2);
                }
            }
            "--fps" => match args.next().as_deref().and_then(Fps::de_nome) {
                Some(f) => {
                    fps = f;
                    opcoes.fps_mxf = f.nome().into();
                }
                None => {
                    eprintln!("fps inválido\n{USO}");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USO}");
                return ExitCode::SUCCESS;
            }
            _ => arquivos.push(PathBuf::from(a)),
        }
    }
    if arquivos.is_empty() {
        eprintln!("{USO}");
        return ExitCode::from(2);
    }
    let mut ok = true;
    for arq in &arquivos {
        if let Err(e) = rodar(arq, so_inspecionar, fps, &opcoes, espectros.as_ref()) {
            eprintln!("ERRO {}: {e:#}", arq.display());
            ok = false;
        }
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn aplicar(c: &mut Configuracao, opcao: &str, v: &str) -> Result<(), String> {
    match opcao {
        "--niveis" => {
            let n: Vec<f32> = v.split(',').map(|x| x.trim().parse().map_err(|_| format!("nível inválido: {x}"))).collect::<Result<_, _>>()?;
            if n.len() != 3 {
                return Err("--niveis precisa de 3 valores".into());
            }
            for (ver, db) in c.versoes.iter_mut().zip(n) {
                ver.atenuacao_db = db;
            }
        }
        "--unica" => {
            let db: f32 = v.parse().map_err(|_| format!("intensidade inválida: {v}"))?;
            let suf = format!("DF{db}");
            c.versoes = vec![Versao { id: "df".into(), ativo: true, atenuacao_db: db, sufixo: suf.clone() }];
            c.residual = Residual { ativo: true, base: "df".into(), sufixo: format!("{suf}_retirado") };
        }
        "--pf" => c.avancado.pos_filtro = v.parse().map_err(|_| format!("pós-filtro inválido: {v}"))?,
        "--limiar" => c.avancado.limiar_ruido_db = v.parse().map_err(|_| format!("limiar inválido: {v}"))?,
        "--formato" => {
            c.formato = match v {
                "wav24" => FormatoSaida::Wav24,
                "wav16" => FormatoSaida::Wav16,
                "mp3" => FormatoSaida::Mp3,
                "mxf" => FormatoSaida::Mxf,
                _ => return Err(format!("formato inválido: {v}")),
            }
        }
        "--canais" => {
            c.canais = match v {
                "mono_por_canal" => ModoCanais::MonoPorCanal,
                "mono_soma" => ModoCanais::MonoSoma,
                "igual_original" => ModoCanais::IgualOriginal,
                "estereo" => ModoCanais::Estereo,
                _ => return Err(format!("modo de canais inválido: {v}")),
            }
        }
        _ => c.taxa = Some(v.parse().map_err(|_| format!("taxa inválida: {v}"))?),
    }
    Ok(())
}

fn rodar(arq: &PathBuf, so_inspecionar: bool, fps: Fps, opcoes: &Configuracao, espectros: Option<&PathBuf>) -> anyhow::Result<()> {
    let (insp, _) = inspecionar(arq)?;
    println!("== {}", arq.display());
    println!(
        "   {} Hz, {} canal(is), {}, {} amostras ({:.2} s), time_reference = {}",
        insp.sr,
        insp.n_canais,
        insp.formato,
        insp.n_amostras,
        insp.duracao_s,
        insp.time_reference.map_or("ausente".into(), |t| format!("{t} (TC {} @ {} fps)", insp.timecode(fps).unwrap(), fps.nome())),
    );
    for a in insp.avisos() {
        println!("   aviso: {a}");
    }
    if so_inspecionar {
        return Ok(());
    }
    let rel = processar(arq, opcoes, &|p| {
        eprint!("\r   [{:3.0}%] canal {}/{}: {:<40}", p.fracao * 100.0, p.canal, p.n_canais, p.etapa);
    })?;
    eprintln!();
    for a in rel.avisos.iter().skip(insp.avisos().len()) {
        println!("   aviso: {a}");
    }
    for c in &rel.canais {
        println!("   {}: atraso por correlação = {}, {} amostras, time_reference = {}", c.base, c.atraso, c.n_amostras, c.time_reference);
        for f in &c.arquivos {
            println!("     -> {}", f.caminho.display());
        }
        if let Some(pasta) = espectros {
            std::fs::create_dir_all(pasta)?;
            let mut lista = vec![("original", &c.sinais.original)];
            lista.extend(c.sinais.versoes.iter().map(|(id, s)| (id.as_str(), s)));
            lista.extend(c.sinais.residual.as_ref().map(|s| ("residual", s)));
            for (rot, s) in lista {
                let p = pasta.join(format!("{}_espectro_{rot}.png", c.base));
                std::fs::write(&p, espectrograma::calcular(s, 48_000, 1600).png()?)?;
                println!("     espectrograma: {}", p.display());
            }
        }
    }
    Ok(())
}
