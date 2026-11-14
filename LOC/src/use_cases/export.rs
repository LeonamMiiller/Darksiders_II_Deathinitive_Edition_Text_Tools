//! Caso de uso **Export** — extrai os textos de um `.loc` para um TXT.
//!
//! Fidelidade ao `TLoc.Export` do Delphi: percorre as referências na ordem
//! `tabela de textos` -> `tabela de IDs`, descarta repetições na primeira
//! ocorrência, aplica `BreakLine` (escapes `<\n>`/`<\r>`) e grava com BOM
//! UTF-8 + CRLF — sem CRLF após a última linha.

use std::path::PathBuf;

use super::error::UseCaseError;
use super::ports::{BinarySink, BinarySource, ProgressEvent, ProgressReporter};
use crate::domain::loc::LocFile;

/// Entrada do caso de uso **Export**.
#[derive(Debug, Clone)]
pub struct ExportRequest {
    /// Caminho do `.loc` de entrada (apenas para as mensagens de progresso).
    pub loc_path: PathBuf,
    /// Caminho do TXT de saída.
    pub txt_path: PathBuf,
}

/// Portas utilizadas pelo **Export** (injetadas pelo adaptador).
pub struct ExportPorts<'a> {
    /// O `.loc` a ser lido.
    pub loc: &'a mut dyn BinarySource,
    /// Destino do TXT.
    pub txt: &'a mut dyn BinarySink,
    /// Relatório de progresso (console ou canal da GUI).
    pub progress: &'a mut dyn ProgressReporter,
}

/// Resumo do **Export** (também usado pela GUI para exibir o resultado).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// Referências de texto no arquivo (tabela de textos + tabela de IDs).
    pub texts: usize,
    /// Textos únicos gravados no TXT.
    pub unique: usize,
    /// Tamanho do TXT gerado.
    pub bytes: usize,
}

/// Executa o Export: lê o `.loc`, monta o TXT e o grava pela porta.
pub fn execute(
    request: &ExportRequest,
    ports: &mut ExportPorts<'_>,
) -> Result<ExportReport, UseCaseError> {
    ports.progress.report(&ProgressEvent::Info(format!(
        "Exporting texts from {} to {} ...",
        request.loc_path.display(),
        request.txt_path.display()
    )));

    let loc = LocFile::parse(&mut *ports.loc)?;
    let outcome = loc.export_txt();
    ports.progress.report(&ProgressEvent::Texts {
        total: outcome.texts,
        unique: outcome.unique,
    });

    let bytes = outcome.txt.len();
    ports
        .txt
        .write_all(&outcome.txt)
        .map_err(|source| UseCaseError::adapter("writing the TXT file", source.into()))?;

    ports.progress.report(&ProgressEvent::Done {
        summary: format!(
            "{} text(s) exported, {} unique, {bytes} byte(s)",
            outcome.texts, outcome.unique
        ),
    });
    Ok(ExportReport {
        texts: outcome.texts,
        unique: outcome.unique,
        bytes,
    })
}
