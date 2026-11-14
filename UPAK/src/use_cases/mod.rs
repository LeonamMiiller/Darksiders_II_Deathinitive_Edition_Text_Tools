//! Casos de uso (aplicação): **Dump** (exportação) e **Insert** (importação).
//!
//! Esta camada orquestra as entidades de [`crate::domain`] por meio das
//! portas definidas em [`ports`], permanecendo livre de detalhes de
//! infraestrutura (sistema de arquivos, zlib, GUI): tudo entra e sai por
//! traits, e as falhas viram [`error::UseCaseError`] — nunca panic.
//!
//! ```text
//! interface_adapters (CLI/GUI)
//!         |  CliArgs / eventos de progresso
//!         v
//! use_cases::{dump, insert}  --ports-->  infrastructure (E/S, zlib)
//!         |
//!         v
//! domain::{manifest, obpk, binary}
//! ```

pub mod blob;
pub mod dump;
pub mod error;
pub mod insert;
pub mod ports;

use std::path::Path;

use crate::domain::error::DomainError;

use self::error::UseCaseError;

/// Extrai o nome do pak a partir do caminho do arquivo `.upak`.
///
/// Equivalente a `ChangeFileExt(ExtractFileName(FileName), '')` do Delphi:
/// `media/media.upak` -> `media`. O valor é devolvido em bytes crus para
/// comparação byte a byte com os nomes do manifesto.
pub fn pak_name_from_path(path: &Path) -> Result<Vec<u8>, UseCaseError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            DomainError::invalid_data(format!(
                "cannot extract the pak name from '{}'",
                path.display()
            ))
        })?;
    Ok(stem.as_bytes().to_vec())
}
