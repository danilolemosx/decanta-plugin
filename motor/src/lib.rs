//! Motor do "Tratamento de Áudio 2": protocolo de limpeza de diálogo para o Avid.
//!
//! DeepFilterNet v0.5.6 em 3 níveis (12 = leve, 20 = médio, 100 = forte) + residual
//! (original − forte), exportados em WAV PCM 24-bit com chunk bext e o time_reference original.
//! Nenhum outro processamento é aplicado.

pub mod analise;
pub mod config;
pub mod deepfilter;
pub mod espectrograma;
mod magma;
pub mod mp3;
pub mod mxf;
pub mod timecode;
pub mod wav;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use ndarray::ArrayView2;
use serde::Serialize;

pub use analise::Silencio;
pub use config::{Avancado, Configuracao, FormatoSaida, ModoCanais, Residual, Versao};
pub use timecode::Fps;
pub use wav::{Formato, Wav};

/// Os 3 níveis do protocolo: (atten_lim_db, rótulo usado no nome do arquivo).
pub const NIVEIS: [(f32, &str); 3] = [(12.0, "leve"), (20.0, "medio"), (100.0, "forte")];
/// Rótulo do residual (original − forte).
pub const ROTULO_RESIDUAL: &str = "passos_e_ambiente_removidos";
/// Janela usada para conferir o atraso por correlação (segundos).
pub const JANELA_CORRELACAO_S: usize = 20;

/// Informações de um canal na inspeção.
#[derive(Debug, Clone, Serialize)]
pub struct InfoCanal {
    pub clip: usize,
    pub silencios: Vec<Silencio>,
}

/// Resultado da inspeção de um WAV.
#[derive(Debug, Clone, Serialize)]
pub struct Inspecao {
    pub caminho: PathBuf,
    /// Nome do arquivo sem extensão.
    pub nome: String,
    pub sr: u32,
    pub n_canais: usize,
    pub formato: String,
    pub n_amostras: usize,
    pub duracao_s: f64,
    pub time_reference: Option<u64>,
    pub canais: Vec<InfoCanal>,
    pub precisa_reamostrar: bool,
}

impl Inspecao {
    /// Avisos para o usuário (clip, silêncio, canais, reamostragem, falta de bext).
    pub fn avisos(&self) -> Vec<String> {
        let mut av = Vec::new();
        if self.time_reference.is_none() {
            av.push("sem chunk bext: time_reference ausente (será gravado 0)".into());
        }
        if self.precisa_reamostrar {
            av.push(format!("{} Hz: será reamostrado para 48 kHz e o time_reference ajustado", self.sr));
        }
        if self.n_canais > 1 {
            av.push(format!("{} canais: cada canal será processado em mono (_ch1, _ch2...)", self.n_canais));
        }
        for (i, c) in self.canais.iter().enumerate() {
            let pre = if self.n_canais > 1 { format!("canal {}: ", i + 1) } else { String::new() };
            if c.clip > 0 {
                av.push(format!("{pre}{} amostras clipadas (|x| ≥ 0,999)", c.clip));
            }
            for s in &c.silencios {
                let dur = format!("{:.2}", s.duracao as f64 / self.sr as f64).replace('.', ",");
                av.push(format!(
                    "{pre}silêncio digital de {dur} s em {}",
                    formatar_tempo(s.inicio as f64 / self.sr as f64)
                ));
            }
        }
        av
    }

    /// time_reference formatado como timecode no fps dado.
    pub fn timecode(&self, fps: Fps) -> Option<String> {
        self.time_reference.map(|tr| timecode::timecode(tr, self.sr, fps))
    }
}

fn formatar_tempo(s: f64) -> String {
    format!("{}:{:06.3}", (s / 60.0).floor() as u64, s % 60.0)
}

/// Lê e inspeciona um WAV.
pub fn inspecionar(caminho: &Path) -> Result<(Inspecao, Wav)> {
    let w = wav::ler(caminho)?;
    let nome = caminho
        .file_stem()
        .context("nome de arquivo inválido")?
        .to_string_lossy()
        .into_owned();
    let canais = w
        .canais
        .iter()
        .map(|c| InfoCanal { clip: analise::contar_clip(c), silencios: analise::silencios(c, w.sr) })
        .collect();
    let insp = Inspecao {
        caminho: caminho.to_path_buf(),
        nome,
        sr: w.sr,
        n_canais: w.canais.len(),
        formato: w.formato.descricao(),
        n_amostras: w.n_amostras(),
        duracao_s: w.n_amostras() as f64 / w.sr as f64,
        time_reference: w.time_reference,
        canais,
        precisa_reamostrar: w.sr != deepfilter::SR_MODELO,
    };
    Ok((insp, w))
}

/// Progresso do processamento de um arquivo.
#[derive(Debug, Clone, Serialize)]
pub struct Progresso {
    pub canal: usize,
    pub n_canais: usize,
    pub etapa: String,
    /// Fração total do arquivo (0..1).
    pub fracao: f32,
}

/// Um arquivo gravado (e, no caso de WAV, relido e conferido).
#[derive(Debug, Clone, Serialize)]
pub struct ArquivoSaida {
    /// "leve", "medio", "forte" ou "residual".
    pub rotulo: String,
    pub caminho: PathBuf,
    pub n_amostras: usize,
    pub sr: u32,
    pub n_canais: usize,
    /// `None` em MP3 (sem timecode).
    pub time_reference: Option<u64>,
}

/// Sinais a 48 kHz de um grupo (primeiro canal), para verificação (espectrogramas e player).
#[derive(Debug, Clone, Default)]
pub struct Sinais {
    pub original: Vec<f32>,
    /// (id da versão, sinal), só das versões processadas.
    pub versoes: Vec<(String, Vec<f32>)>,
    pub residual: Option<Vec<f32>>,
}

impl Sinais {
    pub fn versao(&self, id: &str) -> Option<&Vec<f32>> {
        self.versoes.iter().find(|(v, _)| v == id).map(|(_, s)| s)
    }
}

/// Resultado de um grupo de saída (um arquivo por versão).
#[derive(Debug, Clone, Serialize)]
pub struct ResultadoCanal {
    /// Nome-base das saídas (`<nome>` ou `<nome>_chN`).
    pub base: String,
    pub n_amostras: usize,
    pub sr: u32,
    pub time_reference: u64,
    /// Atraso medido por correlação nos primeiros 20 s (deve ser 0).
    pub atraso: i64,
    pub arquivos: Vec<ArquivoSaida>,
    #[serde(skip)]
    pub sinais: Sinais,
}

#[derive(Debug, Clone, Serialize)]
pub struct Relatorio {
    pub inspecao: Inspecao,
    pub avisos: Vec<String>,
    pub no_protocolo: bool,
    pub canais: Vec<ResultadoCanal>,
}

/// Leva um sinal mono para 48 kHz, como é entregue ao DeepFilterNet e usado no residual.
pub fn para_48k(x: &[f32], sr: u32, int16: bool) -> Result<Vec<f32>> {
    let alvo = deepfilter::SR_MODELO;
    if sr == alvo {
        return Ok(x.to_vec());
    }
    let r = reamostrar(x, sr, alvo)?;
    Ok(if int16 {
        // como um WAV 16-bit reamostrado (o binário só lê int16)
        r.iter().map(|&x| (x * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0).collect()
    } else {
        r
    })
}

/// Canal `canal` do original a 48 kHz.
pub fn original_48k(w: &Wav, canal: usize) -> Result<Vec<f32>> {
    let x = w.canais.get(canal).context("canal inexistente")?;
    para_48k(x, w.sr, w.formato == Formato::Inteiro(16))
}

fn reamostrar(x: &[f32], de: u32, para: u32) -> Result<Vec<f32>> {
    if de == para {
        return Ok(x.to_vec());
    }
    let v = ArrayView2::from_shape((1, x.len()), x)?;
    Ok(df::transforms::resample(v, de as usize, para as usize, None)
        .map_err(|e| anyhow::anyhow!("falha na reamostragem: {e}"))?
        .into_raw_vec())
}

/// Sinal mono de entrada conforme o modo de canais (MonoSoma = média dos canais).
fn sinal_mono(w: &Wav, modo: ModoCanais, canal: usize) -> Result<(Vec<f32>, bool)> {
    let int16 = w.formato == Formato::Inteiro(16);
    if modo == ModoCanais::MonoSoma && w.canais.len() > 1 {
        let n = w.n_amostras();
        let k = w.canais.len() as f64;
        let soma = (0..n).map(|i| (w.canais.iter().map(|c| c[i] as f64).sum::<f64>() / k) as f32).collect();
        Ok((soma, false))
    } else {
        Ok((w.canais.get(canal).context("canal inexistente")?.clone(), int16))
    }
}

/// Grupo de saída: um nome-base e os sinais mono (índices em `fontes`) que viram os canais do arquivo.
struct Grupo {
    base: String,
    canais: Vec<usize>,
}

/// Processa um WAV com a configuração dada (o padrão é o protocolo aprovado).
pub fn processar(
    caminho: &Path,
    cfg: &Configuracao,
    progresso: &(dyn Fn(Progresso) + Sync),
) -> Result<Relatorio> {
    cfg.validar()?;
    let (insp, w) = inspecionar(caminho)?;
    let mut avisos = insp.avisos();
    let pasta = match &cfg.pasta_saida {
        Some(p) => p.clone(),
        None => caminho.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    std::fs::create_dir_all(&pasta)
        .with_context(|| format!("não foi possível criar a pasta {}", pasta.display()))?;

    let sr48 = deepfilter::SR_MODELO;
    let mut sr_out = cfg.taxa.unwrap_or(insp.sr);
    if cfg.formato == FormatoSaida::Mp3 && sr_out > 48_000 {
        avisos.push(format!("MP3 não aceita {sr_out} Hz: saída em 48 kHz"));
        sr_out = 48_000;
    }
    if cfg.formato == FormatoSaida::Mxf && sr_out != 48_000 {
        avisos.push(format!("MXF do Avid exige 48 kHz: saída em 48 kHz (original {sr_out} Hz)"));
        sr_out = 48_000;
    }
    let tr_orig = insp.time_reference.unwrap_or(0);
    let tr = if sr_out == insp.sr {
        tr_orig
    } else {
        let novo = ((tr_orig as u128 * sr_out as u128 + insp.sr as u128 / 2) / insp.sr as u128) as u64;
        avisos.push(format!("taxa de saída {sr_out} Hz: time_reference ajustado de {tr_orig} para {novo}"));
        novo
    };
    if cfg.formato == FormatoSaida::Mp3 {
        avisos.push("MP3 não guarda timecode".into());
    }
    let fps_mxf = Fps::de_nome(&cfg.fps_mxf).unwrap_or(Fps::F23_976);
    if cfg.formato == FormatoSaida::Mxf {
        let sobra = mxf::sobra_do_quadro(tr, sr_out, fps_mxf);
        if sobra > 0 {
            avisos.push(format!(
                "timecode de origem fora do quadro a {} fps: o MXF começa no quadro anterior, com {sobra} amostras de silêncio (sincronia mantida)",
                fps_mxf.nome()
            ));
        }
    }

    // fontes mono a processar e grupos de saída
    let n_ch = w.canais.len();
    let mut fontes: Vec<(Vec<f32>, bool)> = Vec::new();
    let mut grupos: Vec<Grupo> = Vec::new();
    match cfg.canais {
        ModoCanais::MonoPorCanal => {
            for c in 0..n_ch {
                fontes.push(sinal_mono(&w, cfg.canais, c)?);
                let base = if n_ch > 1 { format!("{}_ch{}", insp.nome, c + 1) } else { insp.nome.clone() };
                grupos.push(Grupo { base, canais: vec![c] });
            }
        }
        ModoCanais::MonoSoma => {
            fontes.push(sinal_mono(&w, cfg.canais, 0)?);
            if n_ch > 1 {
                avisos.push(format!("{n_ch} canais somados num único mono antes de processar"));
            }
            grupos.push(Grupo { base: insp.nome.clone(), canais: vec![0] });
        }
        ModoCanais::IgualOriginal => {
            for c in 0..n_ch {
                fontes.push(sinal_mono(&w, cfg.canais, c)?);
            }
            grupos.push(Grupo { base: insp.nome.clone(), canais: (0..n_ch).collect() });
        }
        ModoCanais::Estereo => {
            fontes.push(sinal_mono(&w, cfg.canais, 0)?);
            if n_ch == 1 {
                grupos.push(Grupo { base: insp.nome.clone(), canais: vec![0, 0] });
            } else {
                fontes.push(sinal_mono(&w, cfg.canais, 1)?);
                if n_ch > 2 {
                    avisos.push(format!("{n_ch} canais: o estéreo usa só os canais 1 e 2"));
                }
                grupos.push(Grupo { base: insp.nome.clone(), canais: vec![0, 1] });
            }
        }
    }
    if cfg.formato == FormatoSaida::Mp3 && grupos.iter().any(|g| g.canais.len() > 2) {
        bail!("MP3 aceita só mono ou estéreo; escolha outro modo de canais");
    }

    // processa cada fonte mono: versões necessárias em paralelo + residual
    let idx = cfg.a_processar();
    let n_fontes = fontes.len();
    struct Processado {
        original: Vec<f32>,
        versoes: Vec<(usize, Vec<f32>)>,
        residual: Option<Vec<f32>>,
        atraso: i64,
    }
    let mut processados = Vec::with_capacity(n_fontes);
    for (k, (x, int16)) in fontes.iter().enumerate() {
        let avisar = |etapa: &str, f: f32| {
            progresso(Progresso {
                canal: k + 1,
                n_canais: n_fontes,
                etapa: etapa.into(),
                fracao: (k as f32 + 0.9 * f) / n_fontes as f32,
            })
        };
        if insp.sr != sr48 {
            avisar("reamostrando para 48 kHz", 0.0);
        }
        let original = para_48k(x, insp.sr, *int16)?;
        let n = original.len();
        let entrada = deepfilter::escala_de_entrada(&original, *int16);
        let nomes: Vec<&str> = idx.iter().map(|&i| cfg.versoes[i].id.as_str()).collect();
        let etapa = format!("DeepFilterNet ({})", nomes.join(", "));
        let fracoes = Mutex::new(vec![0f32; idx.len()]);
        let saidas: Vec<Result<Vec<f32>>> = std::thread::scope(|s| {
            let hs: Vec<_> = idx
                .iter()
                .enumerate()
                .map(|(j, &i)| {
                    let nivel = cfg.versoes[i].atenuacao_db;
                    let (entrada, fracoes, avisar, etapa) = (&entrada, &fracoes, &avisar, &etapa);
                    s.spawn(move || {
                        let cb = |f: f32| {
                            let media = {
                                let mut fr = fracoes.lock().unwrap();
                                fr[j] = f;
                                fr.iter().sum::<f32>() / fr.len() as f32
                            };
                            avisar(etapa, 0.95 * media);
                        };
                        deepfilter::executar(entrada, nivel, &cfg.avancado, &cb).map(|y| deepfilter::int16_e_completar(&y, n))
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().expect("thread do DeepFilterNet falhou")).collect()
        });
        let versoes: Vec<(usize, Vec<f32>)> =
            idx.iter().copied().zip(saidas.into_iter().collect::<Result<Vec<_>>>()?).collect();
        let mut atraso = 0;
        for (_, y) in &versoes {
            atraso = analise::atraso_por_correlacao(&original, y, JANELA_CORRELACAO_S * sr48 as usize);
            if atraso != 0 {
                bail!("canal {}: atraso por correlação = {atraso} amostras (esperado 0)", k + 1);
            }
        }
        let residual = if cfg.residual.ativo {
            let base = versoes.iter().find(|(i, _)| cfg.versoes[*i].id == cfg.residual.base).map(|(_, y)| y);
            let base = base.context("versão base do residual não processada")?;
            Some(original.iter().zip(base).map(|(&o, &f)| (o as f64 - f as f64) as f32).collect())
        } else {
            None
        };
        processados.push(Processado { original, versoes, residual, atraso });
    }

    // grava os arquivos
    let ext = cfg.formato.extensao();
    let n_out = |n48: usize| -> usize {
        if sr_out == sr48 {
            n48
        } else if sr_out == insp.sr {
            insp.n_amostras
        } else {
            ((insp.n_amostras as u128 * sr_out as u128 + insp.sr as u128 / 2) / insp.sr as u128) as usize
        }
    };
    let saida_de = |y: &[f32]| -> Result<Vec<f32>> {
        let mut r = reamostrar(y, sr48, sr_out)?;
        r.resize(n_out(y.len()), 0.0);
        Ok(r)
    };
    let no_protocolo = cfg.no_protocolo();
    let mut canais = Vec::with_capacity(grupos.len());
    for (ig, g) in grupos.iter().enumerate() {
        progresso(Progresso {
            canal: ig + 1,
            n_canais: grupos.len(),
            etapa: format!("gravando {}", ext.to_uppercase()),
            fracao: 0.92 + 0.08 * ig as f32 / grupos.len() as f32,
        });
        // (rótulo, descrição, sufixo, sinal por canal de saída)
        let mut lista: Vec<(String, String, String, Vec<&Vec<f32>>)> = Vec::new();
        for &i in &idx {
            let v = &cfg.versoes[i];
            if !v.ativo {
                continue;
            }
            let sinais = g
                .canais
                .iter()
                .map(|&c| &processados[c].versoes.iter().find(|(j, _)| *j == i).unwrap().1)
                .collect();
            // descrição do bext: igual à do protocolo quando a intensidade é uma das dele
            let desc = match config::nivel_do_protocolo(v.atenuacao_db) {
                Some(nivel) if cfg.avancado.padrao() => format!("{} DF {nivel}", g.base),
                _ => format!("{} DF {} dB", g.base, v.atenuacao_db),
            };
            lista.push((v.id.clone(), desc, v.sufixo.clone(), sinais));
        }
        if cfg.residual.ativo {
            let sinais = g.canais.iter().map(|&c| processados[c].residual.as_ref().unwrap()).collect();
            let b = cfg.versao(&cfg.residual.base).unwrap();
            let desc = if b.atenuacao_db == 100.0 && cfg.avancado.padrao() {
                format!("{} DF {ROTULO_RESIDUAL}", g.base)
            } else {
                format!("{} DF retirado (original - {} dB)", g.base, b.atenuacao_db)
            };
            lista.push(("residual".into(), desc, cfg.residual.sufixo.clone(), sinais));
        }

        let mut arquivos = Vec::new();
        let mut n_final = 0;
        for (rotulo, desc, sufixo, sinais) in lista {
            let nome_saida = format!("{}_{}", g.base, sufixo);
            let mut destino = pasta.join(format!("{nome_saida}.{ext}"));
            // reamostra uma vez por fonte distinta (estéreo de mono repete a mesma)
            let mut convertidos: Vec<(*const Vec<f32>, Vec<f32>)> = Vec::new();
            for s in &sinais {
                let p = *s as *const Vec<f32>;
                if !convertidos.iter().any(|(q, _)| *q == p) {
                    convertidos.push((p, saida_de(s)?));
                }
            }
            let fatias: Vec<&[f32]> = sinais
                .iter()
                .map(|s| convertidos.iter().find(|(q, _)| *q == *s as *const Vec<f32>).unwrap().1.as_slice())
                .collect();
            let n = fatias[0].len();
            n_final = n;
            let tr_arquivo = match cfg.formato {
                FormatoSaida::Mp3 => {
                    mp3::gravar(&destino, &fatias, sr_out)?;
                    None
                }
                FormatoSaida::Mxf => {
                    let bmx = cfg.raw2bmx.as_deref().context("raw2bmx não configurado")?;
                    // o clipe começa no quadro inteiro anterior; silêncio no início mantém a sincronia
                    let sobra = mxf::sobra_do_quadro(tr, sr_out, fps_mxf);
                    let tr_quadro = tr - sobra;
                    // e silêncio no fim até fechar o último quadro (o MXF do Avid só guarda quadros inteiros)
                    let total = mxf::completar_quadro(sobra as usize + n, sr_out, fps_mxf);
                    let com_pre: Vec<Vec<f32>> = fatias
                        .iter()
                        .map(|c| {
                            let mut v: Vec<f32> =
                                std::iter::repeat(0.0).take(sobra as usize).chain(c.iter().copied()).collect();
                            v.resize(total, 0.0);
                            v
                        })
                        .collect();
                    let com_pre: Vec<&[f32]> = com_pre.iter().map(Vec::as_slice).collect();
                    let feitos = mxf::gravar(
                        bmx,
                        &pasta.join(&nome_saida),
                        &com_pre,
                        sr_out,
                        tr_quadro,
                        fps_mxf,
                        &nome_saida,
                        &insp.nome,
                    )?;
                    destino = feitos[0].clone();
                    Some(tr_quadro)
                }
                f => {
                    let bits = if f == FormatoSaida::Wav16 { 16 } else { 24 };
                    wav::gravar_bext(&destino, &fatias, bits, sr_out, tr, &desc)?;
                    // conferência pós-gravação
                    let lido = wav::ler(&destino)?;
                    if lido.n_amostras() != n
                        || lido.time_reference != Some(tr)
                        || lido.sr != sr_out
                        || lido.canais.len() != fatias.len()
                    {
                        bail!(
                            "conferência falhou em {}: {} amostras (esperado {n}), time_reference {:?} (esperado {tr})",
                            destino.display(),
                            lido.n_amostras(),
                            lido.time_reference
                        );
                    }
                    Some(tr)
                }
            };
            arquivos.push(ArquivoSaida {
                rotulo,
                caminho: destino,
                n_amostras: n,
                sr: sr_out,
                n_canais: fatias.len(),
                time_reference: tr_arquivo,
            });
        }
        let p0 = &processados[g.canais[0]];
        canais.push(ResultadoCanal {
            base: g.base.clone(),
            n_amostras: n_final,
            sr: sr_out,
            time_reference: tr,
            atraso: p0.atraso,
            arquivos,
            sinais: Sinais {
                original: p0.original.clone(),
                versoes: p0.versoes.iter().map(|(i, y)| (cfg.versoes[*i].id.clone(), y.clone())).collect(),
                residual: p0.residual.clone(),
            },
        });
    }
    progresso(Progresso { canal: grupos.len(), n_canais: grupos.len(), etapa: "concluído".into(), fracao: 1.0 });
    Ok(Relatorio { inspecao: insp, avisos, no_protocolo, canais })
}

/// Trecho processado para a prévia (tudo a 48 kHz, mesmo tamanho).
#[derive(Debug, Clone)]
pub struct Previa {
    pub original: Vec<f32>,
    pub processado: Vec<f32>,
    pub residual: Vec<f32>,
    /// Início do trecho, em amostras a 48 kHz.
    pub inicio: usize,
}

/// Processa só um trecho (com 2 s de pré-rolagem para o modelo "aquecer"), para ouvir uma intensidade.
/// `canal` é ignorado no modo MonoSoma.
pub fn previa(
    w: &Wav,
    modo: ModoCanais,
    canal: usize,
    inicio_s: f64,
    dur_s: f64,
    atten_db: f32,
    av: &Avancado,
) -> Result<Previa> {
    let (x, int16) = sinal_mono(w, modo, canal.min(w.canais.len().saturating_sub(1)))?;
    let sr = deepfilter::SR_MODELO as f64;
    // só a janela necessária (+ margens) vai para 48 kHz
    let margem_s = 2.0;
    let ini_src = ((inicio_s - margem_s).max(0.0) * w.sr as f64) as usize;
    let fim_src = (((inicio_s + dur_s + 0.5) * w.sr as f64) as usize).min(x.len());
    if ini_src >= fim_src {
        bail!("trecho fora do arquivo");
    }
    let janela = para_48k(&x[ini_src..fim_src], w.sr, int16)?;
    let entrada = deepfilter::escala_de_entrada(&janela, int16);
    let y = deepfilter::executar(&entrada, atten_db, av, &|_| {})?;
    let y = deepfilter::int16_e_completar(&y, janela.len());
    let pre = ((inicio_s * sr) as usize).saturating_sub((ini_src as f64 * sr / w.sr as f64) as usize);
    let n = ((dur_s * sr) as usize).min(janela.len().saturating_sub(pre + 1440));
    let original = janela[pre..pre + n].to_vec();
    let processado = y[pre..pre + n].to_vec();
    let residual = original.iter().zip(&processado).map(|(&o, &f)| (o as f64 - f as f64) as f32).collect();
    Ok(Previa { original, processado, residual, inicio: (inicio_s * sr) as usize })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn int16_trunca_e_completa() {
        let y = deepfilter::int16_e_completar(&[0.5, -0.5, 1.5, -1.5, 0.99999], 7);
        assert_eq!(y.len(), 7);
        assert_eq!(y[0], 16383.0 / 32768.0); // trunc(0.5 * 32767)
        assert_eq!(y[1], -16383.0 / 32768.0);
        assert_eq!(y[2], 32767.0 / 32768.0); // satura
        assert_eq!(y[3], -32768.0 / 32768.0);
        assert_eq!(y[4], 32766.0 / 32768.0);
        assert_eq!(&y[5..], &[0.0, 0.0]);
    }
}
