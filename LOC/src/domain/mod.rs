//! Camada de domínio: entidade [`loc::LocFile`] (formato `.loc` da
//! Darksiders II), regras de texto do TXT de tradução e primitivas de
//! leitura/escrita binária.
//!
//! Esta camada não conhece sistema de arquivos nem GUI: tudo trabalha sobre
//! `Read + Seek`/`Write + Seek` genéricos e fatias de `&[u8]`, o que a
//! mantém determinística e testável.

pub mod binary;
pub mod error;
pub mod loc;
pub mod text;

pub use error::DomainError;
