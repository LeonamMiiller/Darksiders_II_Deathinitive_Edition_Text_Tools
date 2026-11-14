//! Erro tipado dos casos de uso.
//!
//! Duas origens possíveis: uma falha de **domínio** (parsing/regras do
//! formato `.loc`) ou uma falha reportada por um **adaptador** através de
//! uma porta (E/S de arquivos...). Na fronteira (CLI/GUI) o erro é exibido
//! pelo `Display` ou encapsulado em `Box<dyn std::error::Error>`.

use std::fmt;

use super::ports::PortError;
use crate::domain::DomainError;

/// Falha durante a execução de um caso de uso.
#[derive(Debug)]
pub enum UseCaseError {
    /// Regra de domínio violada ou estrutura binária inválida.
    Domain(DomainError),
    /// Falha reportada por um adaptador (E/S, GUI...).
    Adapter {
        /// O que a operação estava fazendo (ex.: `writing the TXT file`).
        context: &'static str,
        source: PortError,
    },
}

impl UseCaseError {
    /// Envolve um erro de adaptador com o contexto da operação.
    pub fn adapter(context: &'static str, source: PortError) -> Self {
        Self::Adapter { context, source }
    }
}

impl From<DomainError> for UseCaseError {
    fn from(source: DomainError) -> Self {
        Self::Domain(source)
    }
}

impl From<PortError> for UseCaseError {
    fn from(source: PortError) -> Self {
        Self::Adapter {
            context: "infrastructure operation",
            source,
        }
    }
}

impl fmt::Display for UseCaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(source) => write!(f, "{source}"),
            Self::Adapter { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for UseCaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Domain(source) => Some(source),
            Self::Adapter { source, .. } => Some(source.as_ref()),
        }
    }
}
