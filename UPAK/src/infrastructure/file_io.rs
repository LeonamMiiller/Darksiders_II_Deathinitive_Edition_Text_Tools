//! Adaptadores de E/S sobre o sistema de arquivos: abertura de fontes e
//! sumidouros binários, extração de arquivos (`DumpSink`) e leitura dos
//! payloads de substituição (`PayloadSource`).
//!
//! Os caminhos de domínio do jogo usam `\` (ex.: `media\ui\ui_core`); este
//! módulo converte-os para o separador nativo e protege contra *path
//! traversal* (`..`, caminhos absolutos), uma vez que os nomes vêm de um
//! arquivo binário de terceiros.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::use_cases::ports::{BinarySink, BinarySource, DumpSink, PayloadSource, PortError};

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

/// Converte um caminho de domínio (`\`) em caminho do sistema de arquivos,
/// rejeitando componentes que escapariam da pasta-raiz.
fn resolve(base: &Path, relative: &[u8]) -> Result<PathBuf, PortError> {
    let text = String::from_utf8_lossy(relative);
    let mut path = base.to_path_buf();
    for component in text.split('\\') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." || component.starts_with('/') {
            return Err(format!("illegal path component in '{text}'").into());
        }
        path.push(component);
    }
    Ok(path)
}

/// [`DumpSink`] que grava arquivos dentro de uma pasta-raiz.
pub struct FolderDumpSink {
    base: PathBuf,
}

impl FolderDumpSink {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }
}

impl DumpSink for FolderDumpSink {
    fn write_file(&mut self, relative_path: &[u8], data: &[u8]) -> Result<(), PortError> {
        let path = resolve(&self.base, relative_path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| -> PortError {
                format!("cannot create '{}': {source}", parent.display()).into()
            })?;
        }
        std::fs::write(&path, data)
            .map_err(|source| format!("cannot write '{}': {source}", path.display()).into())
    }
}

/// [`PayloadSource`] que lê arquivos `.loc`/`.gfx` de uma pasta-raiz
/// (a mesma usada pelo Dump).
pub struct FolderPayloadSource {
    base: PathBuf,
}

impl FolderPayloadSource {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }
}

impl PayloadSource for FolderPayloadSource {
    fn read_file(&mut self, relative_path: &[u8]) -> Result<Vec<u8>, PortError> {
        let path = resolve(&self.base, relative_path)?;
        std::fs::read(&path)
            .map_err(|source| format!("cannot read '{}': {source}", path.display()).into())
    }
}
