//! Erros tipados do domínio.
//!
//! Nenhum `unwrap`/`expect` é usado no código de produção: falhas viram
//! `DomainError` e são propagadas com `?`, sendo erros na fronteira
//! (CLI/GUI) encapsulados em `Box<dyn std::error::Error>`.

use std::fmt;

/// Erro produzido pela camada de domínio (parsing binário e regras do contêiner).
#[derive(Debug)]
pub enum DomainError {
    /// Falha de E/S ao ler/escrever uma estrutura binária.
    Io {
        context: &'static str,
        source: std::io::Error,
    },
    /// O stream terminou antes de a estrutura ser lida por completo.
    UnexpectedEof {
        context: &'static str,
    },
    /// O magic number não corresponde ao esperado pelo formato.
    InvalidMagic {
        context: &'static str,
        expected: String,
        found: String,
    },
    /// Um valor lido do arquivo está fora da faixa admissível.
    OutOfRange {
        context: &'static str,
        value: u64,
        limit: u64,
    },
    /// Elemento obrigatório inexistente (pak, arquivo, linha de texto...).
    NotFound {
        context: &'static str,
        detail: String,
    },
    /// Estrutura reconhecida, porém não suportada por esta ferramenta.
    Unsupported {
        detail: String,
    },
    /// Bytes inconsistentes (contagens/offsets impossíveis).
    InvalidData {
        detail: String,
    },
}

impl DomainError {
    pub fn invalid_data(detail: impl Into<String>) -> Self {
        Self::InvalidData {
            detail: detail.into(),
        }
    }

    pub fn not_found(context: &'static str, detail: impl Into<String>) -> Self {
        Self::NotFound {
            context,
            detail: detail.into(),
        }
    }

    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self::Unsupported {
            detail: detail.into(),
        }
    }

    pub fn out_of_range(context: &'static str, value: u64, limit: u64) -> Self {
        Self::OutOfRange {
            context,
            value,
            limit,
        }
    }

    pub fn invalid_magic(context: &'static str, expected: impl Into<String>, found: impl Into<String>) -> Self {
        Self::InvalidMagic {
            context,
            expected: expected.into(),
            found: found.into(),
        }
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::UnexpectedEof { context } => {
                write!(f, "unexpected end of file while reading {context}")
            }
            Self::InvalidMagic {
                context,
                expected,
                found,
            } => write!(f, "invalid magic for {context}: expected {expected}, found {found}"),
            Self::OutOfRange {
                context,
                value,
                limit,
            } => write!(f, "{context}: value {value} exceeds the limit of {limit}"),
            Self::NotFound { context, detail } => write!(f, "{context}: {detail}"),
            Self::Unsupported { detail } => write!(f, "unsupported structure: {detail}"),
            Self::InvalidData { detail } => write!(f, "invalid data: {detail}"),
        }
    }
}

impl std::error::Error for DomainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DomainError {
    fn from(source: std::io::Error) -> Self {
        Self::Io {
            context: "file I/O",
            source,
        }
    }
}
