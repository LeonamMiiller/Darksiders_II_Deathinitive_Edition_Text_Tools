//! Adaptadores de E/S sobre o sistema de arquivos: abertura de fontes e
//! sumidouros binários usados pelos casos de uso **Export**/**Import**.
//!
//! Os arquivos são abertos em binário e usados com
//! `std::io::{Read, Write, Seek}` através das portas `BinarySource`/
//! `BinarySink`; nada é lido para memória aqui — quem decide os tamanhos é
//! o caso de uso.

use std::fs::File;
use std::path::Path;

use crate::use_cases::ports::{BinarySink, BinarySource, PortError};

/// Abre um arquivo existente para leitura (binário, sem truncar).
pub fn open_source(path: &Path) -> Result<Box<dyn BinarySource>, PortError> {
    File::open(path)
        .map(|file| Box::new(file) as Box<dyn BinarySource>)
        .map_err(|source| format!("cannot open '{}' for reading: {source}", path.display()).into())
}

/// Cria (ou trunca) um arquivo para escrita, garantindo o diretório pai.
pub fn create_sink(path: &Path) -> Result<Box<dyn BinarySink>, PortError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| -> PortError {
                format!("cannot create the directory '{}': {source}", parent.display()).into()
            })?;
        }
    }
    File::create(path)
        .map(|file| Box::new(file) as Box<dyn BinarySink>)
        .map_err(|source| format!("cannot create '{}' for writing: {source}", path.display()).into())
}
