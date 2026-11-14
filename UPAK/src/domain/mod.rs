//! Camada de domínio: entidades e regras de negócio puras dos formatos
//! MANIFEST.BIN e OBPK/UPAK, além das primitivas de leitura/escrita binária.
//!
//! Esta camada não conhece sistema de arquivos, compressão nem GUI: todos os
//! tipos trabalham sobre `Read + Seek`/`Write + Seek` genéricos, o que a
//! mantém determinística e testável.

pub mod binary;
pub mod error;
pub mod manifest;
pub mod obpk;

pub use error::DomainError;
