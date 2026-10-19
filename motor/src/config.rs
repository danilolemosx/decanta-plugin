//! Configuração do processamento: versões a gerar, intensidades, sufixos e formato de saída.
//!
//! `Configuracao::default()` é o protocolo aprovado (12 / 20 / 100 dB + residual do forte,
//! WAV 24-bit, mono por canal, taxa original). O app usa uma versão só, com a intensidade
//! escolhida na prévia.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Atenuações do protocolo aprovado (dB): leve, médio, forte.
pub const PROTOCOLO_DB: [f32; 3] = [12.0, 20.0, 100.0];
/// Identificadores das 3 versões, na ordem leve, médio, forte.
pub const IDS: [&str; 3] = ["leve", "medio", "forte"];

/// Uma das versões processadas (leve, médio ou forte).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Versao {
    /// "leve", "medio" ou "forte".
    pub id: String,
    /// Exportar esta versão.
    pub ativo: bool,
    /// Limite de atenuação do DeepFilterNet (0 a 100 dB).
    pub atenuacao_db: f32,
    /// Sufixo do arquivo: `<nome>_<sufixo>`.
    pub sufixo: String,
}

/// Residual = original − uma das versões (por padrão, a forte).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Residual {
    pub ativo: bool,
    /// Id da versão subtraída do original.
    pub base: String,
    pub sufixo: String,
}

/// Ajustes avançados do DeepFilterNet. O padrão é o do binário `deep-filter` (protocolo).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Avancado {
    /// Força do pós-filtro (`--pf-beta`); 0 = desligado (padrão).
    pub pos_filtro: f32,
    /// Abaixo desta relação fala/ruído local (dB) o trecho é tratado como só ruído (`--min-db-thresh`).
    pub limiar_ruido_db: f32,
}

/// `--min-db-thresh` padrão do binário.
pub const LIMIAR_RUIDO_PADRAO: f32 = -15.0;

impl Default for Avancado {
    fn default() -> Self {
        Avancado { pos_filtro: 0.0, limiar_ruido_db: LIMIAR_RUIDO_PADRAO }
    }
}

impl Avancado {
    pub fn padrao(&self) -> bool {
        *self == Avancado::default()
    }
}

/// Nível do protocolo ("leve", "medio", "forte") com esta atenuação, se houver.
pub fn nivel_do_protocolo(db: f32) -> Option<&'static str> {
    PROTOCOLO_DB.iter().position(|&p| p == db).map(|i| IDS[i])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatoSaida {
    /// WAV PCM 24-bit com bext (timecode).
    Wav24,
    /// WAV PCM 16-bit com bext (timecode).
    Wav16,
    /// MP3 320 kbps (sem timecode).
    Mp3,
    /// MXF OP-Atom para o Avid (via bmx/raw2bmx), 48 kHz 24-bit, com timecode.
    Mxf,
}

impl FormatoSaida {
    pub fn extensao(self) -> &'static str {
        match self {
            FormatoSaida::Mp3 => "mp3",
            FormatoSaida::Mxf => "mxf",
            _ => "wav",
        }
    }
}

/// Como os canais do original viram arquivos de saída.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModoCanais {
    /// Um arquivo mono por canal (`_ch1`, `_ch2`...). Mono continua mono.
    MonoPorCanal,
    /// Soma os canais num único mono antes de processar.
    MonoSoma,
    /// Um arquivo com o mesmo número de canais do original (cada canal processado).
    IgualOriginal,
    /// Arquivo estéreo: mono vira L = R; estéreo continua estéreo; com mais canais, usa os 2 primeiros.
    Estereo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Configuracao {
    /// Versões a gerar (no protocolo: leve, médio e forte, nesta ordem; no app: uma só).
    pub versoes: Vec<Versao>,
    pub residual: Residual,
    pub formato: FormatoSaida,
    pub canais: ModoCanais,
    /// Taxa de saída em Hz; `None` = a mesma do original.
    pub taxa: Option<u32>,
    /// `None` = ao lado do original.
    #[serde(default)]
    pub pasta_saida: Option<PathBuf>,
    /// Frame rate do projeto no Avid (MXF): "23.976", "24", "25", "29.97", "29.97df" ou "30".
    #[serde(default = "fps_padrao")]
    pub fps_mxf: String,
    /// Caminho do raw2bmx (bmx), necessário para MXF.
    #[serde(default)]
    pub raw2bmx: Option<PathBuf>,
    #[serde(default)]
    pub avancado: Avancado,
}

fn fps_padrao() -> String {
    "23.976".into()
}

impl Default for Configuracao {
    fn default() -> Self {
        let sufixos = ["DF_leve", "DF_medio", "DF_forte"];
        Configuracao {
            versoes: (0..3)
                .map(|i| Versao {
                    id: IDS[i].into(),
                    ativo: true,
                    atenuacao_db: PROTOCOLO_DB[i],
                    sufixo: sufixos[i].into(),
                })
                .collect(),
            residual: Residual {
                ativo: true,
                base: "forte".into(),
                sufixo: "DF_passos_e_ambiente_removidos".into(),
            },
            formato: FormatoSaida::Wav24,
            canais: ModoCanais::MonoPorCanal,
            taxa: None,
            pasta_saida: None,
            fps_mxf: fps_padrao(),
            raw2bmx: None,
            avancado: Avancado::default(),
        }
    }
}

impl Configuracao {
    /// O processamento é o do protocolo aprovado? Toda versão exportada tem uma intensidade
    /// do protocolo (12, 20 ou 100), o residual (se houver) vem do forte (100) e os ajustes
    /// avançados são os padrões. (Formato, canais, taxa e sufixos não mudam o processamento.)
    pub fn no_protocolo(&self) -> bool {
        self.avancado.padrao()
            && self.versoes.iter().filter(|v| v.ativo).all(|v| nivel_do_protocolo(v.atenuacao_db).is_some())
            && (!self.residual.ativo || self.versao(&self.residual.base).is_some_and(|b| b.atenuacao_db == 100.0))
    }

    pub fn versao(&self, id: &str) -> Option<&Versao> {
        self.versoes.iter().find(|v| v.id == id)
    }

    /// Índices (em `versoes`) que precisam ser processados: as ativas e a base do residual.
    pub fn a_processar(&self) -> Vec<usize> {
        (0..self.versoes.len())
            .filter(|&i| {
                self.versoes[i].ativo || (self.residual.ativo && self.versoes[i].id == self.residual.base)
            })
            .collect()
    }

    pub fn validar(&self) -> anyhow::Result<()> {
        use anyhow::bail;
        if self.versoes.is_empty() || self.versoes.len() > 8 {
            bail!("configuração inválida: de 1 a 8 versões");
        }
        let mut ids: Vec<&str> = self.versoes.iter().map(|v| v.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        if ids.len() != self.versoes.len() || ids.iter().any(|i| i.is_empty()) {
            bail!("configuração inválida: ids de versão vazios ou repetidos");
        }
        if !(0.0..=0.2).contains(&self.avancado.pos_filtro) {
            bail!("pós-filtro fora de 0–0,2");
        }
        if !(-40.0..=10.0).contains(&self.avancado.limiar_ruido_db) {
            bail!("limiar de ruído fora de −40 a 10 dB");
        }
        if self.a_processar().is_empty() {
            bail!("nenhuma versão marcada para exportar");
        }
        let mut sufixos = Vec::new();
        for v in &self.versoes {
            if !(0.0..=100.0).contains(&v.atenuacao_db) {
                bail!("intensidade de {} fora de 0–100 dB", v.id);
            }
            if v.ativo {
                sufixos.push(v.sufixo.as_str());
            }
        }
        if self.residual.ativo {
            if self.versao(&self.residual.base).is_none() {
                bail!("base do residual inválida: {}", self.residual.base);
            }
            sufixos.push(self.residual.sufixo.as_str());
        }
        for s in &sufixos {
            if s.trim().is_empty() {
                bail!("há um sufixo vazio");
            }
            if s.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
                bail!("sufixo com caractere inválido: {s}");
            }
        }
        let mut unicos = sufixos.clone();
        unicos.sort();
        unicos.dedup();
        if unicos.len() != sufixos.len() {
            bail!("dois arquivos teriam o mesmo sufixo");
        }
        if let Some(t) = self.taxa {
            if !(8_000..=192_000).contains(&t) {
                bail!("taxa de saída inválida: {t} Hz");
            }
            if self.formato == FormatoSaida::Mp3 && t > 48_000 {
                bail!("MP3 aceita no máximo 48 kHz");
            }
            if self.formato == FormatoSaida::Mxf && t != 48_000 {
                bail!("MXF para o Avid exige 48 kHz");
            }
        }
        if self.formato == FormatoSaida::Mxf {
            if crate::Fps::de_nome(&self.fps_mxf).is_none() {
                bail!("frame rate inválido para MXF: {}", self.fps_mxf);
            }
            match &self.raw2bmx {
                Some(p) if p.is_file() => {}
                _ => bail!("raw2bmx (bmx) não encontrado: necessário para gerar MXF"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn padrao_e_o_protocolo() {
        let c = Configuracao::default();
        assert!(c.no_protocolo());
        c.validar().unwrap();
        assert_eq!(c.a_processar(), vec![0, 1, 2]);
    }

    #[test]
    fn residual_exige_a_base() {
        let mut c = Configuracao::default();
        for v in &mut c.versoes {
            v.ativo = false;
        }
        assert_eq!(c.a_processar(), vec![2]); // só o forte, para o residual
        c.residual.ativo = false;
        assert!(c.validar().is_err());
        c.versoes[0].ativo = true;
        c.versoes[0].atenuacao_db = 15.0;
        assert!(!c.no_protocolo());
        c.validar().unwrap();
    }

    #[test]
    fn versao_unica() {
        let mut c = Configuracao::default();
        c.versoes = vec![Versao { id: "df".into(), ativo: true, atenuacao_db: 20.0, sufixo: "DF20".into() }];
        c.residual = Residual { ativo: true, base: "df".into(), sufixo: "DF20_retirado".into() };
        c.validar().unwrap();
        assert!(!c.no_protocolo()); // residual do protocolo é do forte
        c.residual.ativo = false;
        assert!(c.no_protocolo());
        c.versoes[0].atenuacao_db = 30.0;
        assert!(!c.no_protocolo());
        c.versoes[0].atenuacao_db = 100.0;
        c.residual.ativo = true;
        assert!(c.no_protocolo());
        c.avancado.pos_filtro = 0.02;
        assert!(!c.no_protocolo());
        c.validar().unwrap();
    }

    #[test]
    fn sufixos_validos() {
        let mut c = Configuracao::default();
        c.versoes[1].sufixo = "DF_leve".into();
        assert!(c.validar().is_err());
        c.versoes[1].sufixo = "a/b".into();
        assert!(c.validar().is_err());
    }
}
