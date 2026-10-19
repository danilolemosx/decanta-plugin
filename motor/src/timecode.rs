//! Conversão do time_reference (amostras desde a meia-noite) em timecode.

use serde::Serialize;

/// Frame rates de timecode suportados.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Fps {
    F23_976,
    F24,
    F25,
    F29_97,
    F29_97Df,
    F30,
}

impl Fps {
    pub const TODOS: [Fps; 6] = [Fps::F23_976, Fps::F24, Fps::F25, Fps::F29_97, Fps::F29_97Df, Fps::F30];

    /// Quadros por segundo como fração (numerador, denominador).
    pub fn fracao(self) -> (u64, u64) {
        self.taxa()
    }

    fn taxa(self) -> (u64, u64) {
        match self {
            Fps::F23_976 => (24_000, 1001),
            Fps::F24 => (24, 1),
            Fps::F25 => (25, 1),
            Fps::F29_97 | Fps::F29_97Df => (30_000, 1001),
            Fps::F30 => (30, 1),
        }
    }

    /// Quadros por segundo de timecode (base da contagem HH:MM:SS:FF).
    fn base(self) -> u64 {
        match self {
            Fps::F23_976 | Fps::F24 => 24,
            Fps::F25 => 25,
            _ => 30,
        }
    }

    pub fn nome(self) -> &'static str {
        match self {
            Fps::F23_976 => "23.976",
            Fps::F24 => "24",
            Fps::F25 => "25",
            Fps::F29_97 => "29.97",
            Fps::F29_97Df => "29.97df",
            Fps::F30 => "30",
        }
    }

    pub fn de_nome(s: &str) -> Option<Fps> {
        Fps::TODOS.into_iter().find(|f| f.nome().eq_ignore_ascii_case(s))
    }
}

/// Converte amostras desde a meia-noite em HH:MM:SS:FF (HH:MM:SS;FF em drop-frame).
pub fn timecode(amostras: u64, sr: u32, fps: Fps) -> String {
    let (num, den) = fps.taxa();
    let mut quadros = (amostras as u128 * num as u128 / (sr as u128 * den as u128)) as u64;
    let base = fps.base();
    if fps == Fps::F29_97Df {
        // drop-frame: pula os quadros 0 e 1 a cada minuto, exceto nos múltiplos de 10
        let d = quadros / 17_982;
        let m = quadros % 17_982;
        quadros += 18 * d + if m > 1 { 2 * ((m - 2) / 1798) } else { 0 };
    }
    let ff = quadros % base;
    let s = quadros / base;
    let sep = if fps == Fps::F29_97Df { ';' } else { ':' };
    format!("{:02}:{:02}:{:02}{sep}{:02}", s / 3600 % 24, s / 60 % 60, s % 60, ff)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn exemplos() {
        // arquivo de teste: 173106934 = 86467 quadros × 2002 amostras (23,976)
        assert_eq!(timecode(173_106_934, 48_000, Fps::F23_976), "01:00:02:19");
        assert_eq!(timecode(173_106_934, 48_000, Fps::F24), "01:00:06:09");
        assert_eq!(timecode(0, 48_000, Fps::F25), "00:00:00:00");
        assert_eq!(timecode(48_000 * 3600, 48_000, Fps::F25), "01:00:00:00");
        // 29,97 DF: 1800 quadros → 00:01:00;02 (quadros 0 e 1 do minuto são pulados)
        assert_eq!(timecode(2_882_880, 48_000, Fps::F29_97Df), "00:01:00;02");
        // 10 minutos em quadros DF = 17982 → 00:10:00;00
        assert_eq!(timecode(28_799_972, 48_000, Fps::F29_97Df), "00:10:00;00");
        assert_eq!(Fps::de_nome("29.97DF"), Some(Fps::F29_97Df));
    }
}
