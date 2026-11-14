//! Orquestração compartilhada pelas interfaces **CLI** e **GUI**.
//!
//! Ponto de composição de ambas as interfaces: abre os arquivos, monta as
//! portas de infraestrutura (E/S, zlib, extração) e executa os casos de uso,
//! devolvendo um resumo em texto. Nenhuma regra de negócio mora aqui — todo
//! o domínio continua em `domain`/`use_cases`.

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::infrastructure::compression::ZlibCodec;
use crate::infrastructure::file_io::{
    create_sink, open_source, FolderDumpSink, FolderPayloadSource,
};
use crate::use_cases::dump::{self, DumpPorts, DumpRequest};
use crate::use_cases::insert::{self, InsertPorts, InsertRequest};
use crate::use_cases::ports::ProgressReporter;

/// Resultado de uma operação: resumo em texto ou erro pronto para exibir.
pub type OperationResult = Result<String, Box<dyn Error + Send + Sync>>;

/// Operação a executar — inferida pela extensão do arquivo de entrada ou
/// forçada pelo usuário (`-o` na CLI, botão na GUI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Exporta os payloads do `.upak` para uma pasta.
    Dump,
    /// Importa os payloads de uma pasta para um novo `.upak`.
    Insert,
}

/// Infere a operação a partir do arquivo de entrada.
///
/// Regra do legado (`RunConsole`): extensão `.upak` -> Dump; qualquer
/// outra -> Insert. Duas correções em relação ao Delphi (documentadas no
/// README): a comparação é *case-insensitive* (o legado comparava com
/// `.UPAK` literal e classificava `media.upak` como Insert) e um arquivo
/// **sem extensão** é recusado em vez de ser tratado como Insert.
pub fn infer_operation(input: &Path) -> Option<Operation> {
    let extension = input.extension()?.to_str()?;
    Some(if extension.eq_ignore_ascii_case("upak") {
        Operation::Dump
    } else {
        Operation::Insert
    })
}

/// `manifest.bin` ao lado do `.upak` (mesma regra do `ExtractFilePath` legado).
pub fn manifest_path(input: &Path) -> PathBuf {
    input.with_file_name("manifest.bin")
}

/// `manifest.bin.NEW` (o legado concatenava `'.NEW'` ao caminho do manifesto).
pub fn manifest_new_path(input: &Path) -> PathBuf {
    let mut path = manifest_path(input).into_os_string();
    path.push(".NEW");
    PathBuf::from(path)
}

/// Caminho do `translation.upak` (ao lado do `.upak` de entrada).
pub fn translation_path(input: &Path) -> PathBuf {
    input.with_file_name("translation.upak")
}

/// **Dump**: extrai os `.loc`/`.gfx` de `input` para `output`.
pub fn dump(input: &Path, output: &Path, progress: &mut dyn ProgressReporter) -> OperationResult {
    let mut manifest = open_source(&manifest_path(input))?;
    let mut pak = open_source(input)?;
    let mut sink = FolderDumpSink::new(output);
    let codec = ZlibCodec::default();

    let report = dump::execute(
        &DumpRequest {
            pak_path: input.to_path_buf(),
        },
        &mut DumpPorts {
            manifest: &mut *manifest,
            pak: &mut *pak,
            codec: &codec,
            sink: &mut sink,
            progress,
        },
    )?;

    Ok(format!(
        "{} file(s) extracted into {} ({} container(s) processed, {} skipped)",
        report.extracted,
        output.display(),
        report.containers,
        report.skipped
    ))
}

/// **Insert**: monta `translation.upak` e `manifest.bin.NEW` a partir dos
/// arquivos de `payload_folder` (saída do Dump).
///
/// Em caso de falha as saídas parciais são removidas, para nunca deixar um
/// arquivo incompleto que possa ser confundido com um resultado válido.
pub fn insert(
    input: &Path,
    payload_folder: &Path,
    progress: &mut dyn ProgressReporter,
) -> OperationResult {
    let translation_path = translation_path(input);
    let manifest_out_path = manifest_new_path(input);

    let result = {
        let mut manifest = open_source(&manifest_path(input))?;
        let mut pak = open_source(input)?;
        let mut payloads = FolderPayloadSource::new(payload_folder);
        let mut translation_out = create_sink(&translation_path)?;
        let mut manifest_out = create_sink(&manifest_out_path)?;
        let codec = ZlibCodec::default();

        insert::execute(
            &InsertRequest {
                pak_path: input.to_path_buf(),
            },
            &mut InsertPorts {
                manifest: &mut *manifest,
                pak: &mut *pak,
                payloads: &mut payloads,
                translation: &mut *translation_out,
                manifest_out: &mut *manifest_out,
                codec: &codec,
                progress,
            },
        )
    };

    match result {
        Ok(report) => Ok(format!(
            "{} container(s) imported; created {} ({} bytes) and {}",
            report.imported,
            translation_path.display(),
            report.translation_size,
            manifest_out_path.display()
        )),
        Err(error) => {
            // Nenhuma saída parcial para trás em caso de falha.
            let _ = std::fs::remove_file(&translation_path);
            let _ = std::fs::remove_file(&manifest_out_path);
            Err(error.into())
        }
    }
}
