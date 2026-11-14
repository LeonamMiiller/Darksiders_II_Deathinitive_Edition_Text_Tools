//! Casos de uso (aplicação): **Export** (`.loc` -> `.txt`) e **Import**
//! (`.txt` -> `.loc.NEW`).
//!
//! Esta camada orquestra a entidade [`crate::domain::loc::LocFile`] por meio
//! das portas definidas em [`ports`], permanecendo livre de detalhes de
//! infraestrutura (sistema de arquivos, GUI): tudo entra e sai por traits, e
//! as falhas viram [`error::UseCaseError`] — nunca panic.
//!
//! ```text
//! interface_adapters (CLI/GUI)
//!         |  CliArgs / eventos de progresso
//!         v
//! use_cases::{export, import}  --ports-->  infrastructure (E/S)
//!         |
//!         v
//! domain::{loc, text, binary}
//! ```

pub mod error;
pub mod export;
pub mod import;
pub mod ports;

use crate::domain::DomainError;

use self::error::UseCaseError;
use self::ports::BinarySource;

/// Teto de leitura dos arquivos de texto (defesa contra entradas
/// corrompidas: nenhum TXT real de tradução chega perto disso).
const MAX_TEXT_FILE: usize = 1 << 29; // 512 MiB

/// Lê um arquivo inteiro através de uma porta, em blocos de 64 KiB e com
/// teto de tamanho.
pub fn read_source(
    source: &mut dyn BinarySource,
    context: &'static str,
) -> Result<Vec<u8>, UseCaseError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = source
            .read(&mut chunk)
            .map_err(|source| UseCaseError::adapter(context, source.into()))?;
        if read == 0 {
            break;
        }
        let total = buffer.len() + read;
        if total > MAX_TEXT_FILE {
            return Err(DomainError::out_of_range(context, total as u64, MAX_TEXT_FILE as u64).into());
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(buffer)
}
