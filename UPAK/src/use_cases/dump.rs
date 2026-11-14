//! Caso de uso **Dump** — exporta os contêineres OBPK de um pacote `.upak`.
//!
//! Fidelidade ao `TUPAK.Export`/`TOBPK.Export` do Delphi, com as correções
//! documentadas no README:
//!
//! * o conteúdo `.gfx` é exportado por completo (o legado truncava);
//! * o tamanho da janela de leitura do blob é limitado pelo tamanho
//!   declarado + margem zlib (o legado lia sem limite).
//!
//! Apenas payloads reconhecidos são gravados: arquivos `.loc` (magic
//! `LOC\x02`) e o conteúdo `.gfx` do contêiner de fontes. Demais formatos
//! (DDS, BOD, Wwise...) são apenas deslocados, como no legado.

use std::path::PathBuf;

use super::blob::{decode_gfx_payload, read_blob};
use super::error::UseCaseError;
use super::pak_name_from_path;
use super::ports::{BlobCodec, BinarySource, DumpSink, ProgressEvent, ProgressReporter};
use crate::domain::binary::BinaryReader;
use crate::domain::error::DomainError;
use crate::domain::manifest::Manifest;
use crate::domain::obpk::{Obpk, PayloadKind};

/// Entrada do caso de uso **Dump**.
#[derive(Debug, Clone)]
pub struct DumpRequest {
    /// Caminho do `.upak` a exportar (o *stem* identifica o pak no manifesto).
    pub pak_path: PathBuf,
}

/// Portas utilizadas pelo **Dump** (todas injetadas pelo adaptador).
pub struct DumpPorts<'a> {
    /// `manifest.bin` do mesmo diretório do `.upak`.
    pub manifest: &'a mut dyn BinarySource,
    /// O próprio `.upak` (7 GB no caso do `media.upak`).
    pub pak: &'a mut dyn BinarySource,
    /// Códec zlib (nível 9).
    pub codec: &'a dyn BlobCodec,
    /// Pasta de saída dos arquivos extraídos.
    pub sink: &'a mut dyn DumpSink,
    /// Relatório de progresso (console ou buffer da GUI).
    pub progress: &'a mut dyn ProgressReporter,
}

/// Resumo do **Dump** (também usado pela GUI para exibir o resultado).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DumpReport {
    /// Contêineres analisados (não ignorados).
    pub containers: usize,
    /// Contêineres ignorados (chunked/vazios/sem dados).
    pub skipped: usize,
    /// Arquivos `.loc`/`.gfx` gravados na saída.
    pub extracted: usize,
}

/// Executa o Dump: lê o manifesto, percorre os contêineres do pak e grava
/// os payloads reconhecidos através do [`DumpSink`].
pub fn execute(request: &DumpRequest, ports: &mut DumpPorts<'_>) -> Result<DumpReport, UseCaseError> {
    ports.progress.report(&ProgressEvent::Info(format!(
        "Exporting files from {} ...",
        request.pak_path.display()
    )));

    let manifest = {
        let mut reader = BinaryReader::new(&mut *ports.manifest);
        Manifest::parse(&mut reader)?
    };

    let pak_name = pak_name_from_path(&request.pak_path)?;
    let pak_index = manifest.pak_index_of(&pak_name).ok_or_else(|| {
        DomainError::not_found(
            "pak",
            format!(
                "'{}' is not listed in manifest.bin",
                String::from_utf8_lossy(&pak_name)
            ),
        )
    })?;

    let file_indices = manifest.files_of_pak(pak_index);
    let total = file_indices.len();
    ports
        .progress
        .report(&ProgressEvent::Info(format!("File count: {total}")));

    let mut pak_reader = BinaryReader::new(&mut *ports.pak);
    let stream_len = pak_reader.stream_len()?;

    let mut report = DumpReport::default();
    for (position, &file_index) in file_indices.iter().enumerate() {
        let file = &manifest.files[file_index];
        // `files_of_pak` só devolve entradas com pak válido -> `Some`.
        let full_path = manifest
            .full_name(file)?
            .ok_or_else(|| DomainError::not_found("file path", "entry without a pak"))?;
        let path_text = String::from_utf8_lossy(&full_path).into_owned();

        ports.progress.report(&ProgressEvent::Container {
            index: position + 1,
            total,
            path: path_text.clone(),
        });

        let obpk = Obpk::parse(&mut pak_reader, file.offset)?;
        if let Some(reason) = obpk.skip_reason() {
            report.skipped += 1;
            ports.progress.report(&ProgressEvent::ContainerSkipped {
                path: path_text,
                reason,
            });
            continue;
        }

        let blob = read_blob(&mut pak_reader, file.offset, &obpk, stream_len, ports.codec)?;
        for plan in obpk.plan_extraction(&blob, &full_path)? {
            if plan.kind == PayloadKind::Skip {
                continue;
            }
            let end = plan.offset + plan.size;
            let payload = blob.get(plan.offset..end).ok_or_else(|| {
                DomainError::invalid_data(format!("entry {} outside the blob", plan.index))
            })?;

            let mut output = full_path.clone();
            output.push(b'\\');
            output.extend_from_slice(&plan.name);
            let extension: &[u8] = match plan.kind {
                PayloadKind::Gfx => b".gfx",
                _ => b".loc",
            };
            output.extend_from_slice(extension);

            match plan.kind {
                PayloadKind::Gfx => {
                    // Conteúdo completo do `.gfx` (desvio documentado).
                    let decoded = decode_gfx_payload(payload, ports.codec)?;
                    ports.sink.write_file(&output, &decoded)?;
                }
                _ => {
                    ports.sink.write_file(&output, payload)?;
                }
            }

            report.extracted += 1;
            ports.progress.report(&ProgressEvent::Extracted {
                path: String::from_utf8_lossy(&output).into_owned(),
            });
        }
        report.containers += 1;
    }

    ports.progress.report(&ProgressEvent::Done {
        summary: format!(
            "{} container(s) processed, {} skipped, {} file(s) extracted",
            report.containers, report.skipped, report.extracted
        ),
    });
    Ok(report)
}
