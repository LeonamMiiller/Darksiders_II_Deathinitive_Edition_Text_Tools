//! Caso de uso **Insert** — importa os arquivos de tradução de volta para
//! um pacote `.upak`.
//!
//! Fidelidade ao `TUPAK.Import`/`TOBPK.Import` do Delphi: gera
//! `translation.upak` (somente os alvos de `FileList`) e `manifest.bin.NEW`
//! (manifesto reescrito com um pak adicional `translation`, `unk = $FF`).
//!
//! Desvios documentados do legado:
//!
//! * `PakCount` passa a ser `original + 1` (o Delphi fixava `3`);
//! * o índice do novo pak é `paks.len()` em vez do fixo `2`;
//! * a inferência da operação/leitura dos payloads não depende de caixa.
//!
//! Cada contêiner-alvo é remontado assim: header + tabelas são copiados
//! byte a byte, o blob é descompactado, as entradas `.loc`/`.gfx` são
//! substituídas pelos arquivos do Dump (os demais payloads são copiados
//! intactos) e o blob é recompactado; em seguida a segunda tabela e a
//! tabela de tamanhos são regravadas com *seek* explícito.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use super::blob::read_blob;
use super::error::UseCaseError;
use super::pak_name_from_path;
use super::ports::{
    BlobCodec, BinarySink, BinarySource, PayloadSource, ProgressEvent, ProgressReporter,
};
use crate::domain::binary::{BinaryReader, BinaryWriter};
use crate::domain::error::DomainError;
use crate::domain::manifest::{
    Manifest, ManifestHeader, ManifestWriter, PakEntry, IMPORT_TARGETS, NO_PAK,
};
use crate::domain::obpk::{is_font_container_path, peek_u32, Obpk, LOC_MAGIC};

/// Entrada do caso de uso **Insert**.
#[derive(Debug, Clone)]
pub struct InsertRequest {
    /// Caminho do `.upak` original (o *stem* identifica o pak no manifesto).
    pub pak_path: PathBuf,
}

/// Portas utilizadas pelo **Insert** (todas injetadas pelo adaptador).
pub struct InsertPorts<'a> {
    /// `manifest.bin` original (lido para reconstrução do `.NEW`).
    pub manifest: &'a mut dyn BinarySource,
    /// O `.upak` original (fonte dos contêineres a regravar).
    pub pak: &'a mut dyn BinarySource,
    /// Arquivos `.loc`/`.gfx` exportados previamente pelo Dump.
    pub payloads: &'a mut dyn PayloadSource,
    /// Destino do novo `translation.upak`.
    pub translation: &'a mut dyn BinarySink,
    /// Destino do `manifest.bin.NEW`.
    pub manifest_out: &'a mut dyn BinarySink,
    /// Códec zlib (nível 9).
    pub codec: &'a dyn BlobCodec,
    /// Relatório de progresso (console ou buffer da GUI).
    pub progress: &'a mut dyn ProgressReporter,
}

/// Resumo do **Insert**.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InsertReport {
    /// Contêineres-alvo regravados no `translation.upak`.
    pub imported: usize,
    /// `PakCount` do manifesto gerado (`original + 1`).
    pub pak_count: u32,
    /// Total de entradas da tabela de arquivos do manifesto gerado.
    pub file_count: u32,
    /// Tamanho final do `translation.upak`.
    pub translation_size: u64,
}

/// Estado de uma entrada remontada do blob.
struct RebuiltEntry {
    /// Novo tamanho da entrada (bytes no blob).
    size: u32,
    /// A entrada foi substituída por um arquivo do Dump?
    replaced: bool,
}

/// Executa o Insert: monta o `translation.upak` e reescreve o manifesto.
pub fn execute(
    request: &InsertRequest,
    ports: &mut InsertPorts<'_>,
) -> Result<InsertReport, UseCaseError> {
    ports.progress.report(&ProgressEvent::Info(format!(
        "Importing files into {} ...",
        request.pak_path.display()
    )));

    // ---- Manifesto original (o stream é reutilizado ao final, para copiar
    // os blocos BOD/Null/Langs/Last) ----
    let mut manifest_reader = BinaryReader::new(&mut *ports.manifest);
    let manifest = Manifest::parse(&mut manifest_reader)?;

    // O bloco BOD deve começar exatamente onde termina a tabela de
    // arquivos; caso contrário, bytes do original seriam perdidos ao copiar
    // os blocos (o Delphi falharia silenciosamente).
    if u64::from(manifest.header.bod_block_offset) != manifest.tables_end {
        return Err(DomainError::invalid_data(format!(
            "manifest.bin is not contiguous: the file table ends at {} but the BOD block starts at {}",
            manifest.tables_end, manifest.header.bod_block_offset
        ))
        .into());
    }

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

    // Índice do novo pak (`translation`): logo após os existentes, nunca
    // `$FF` (reservado para "sem pak").
    let new_pak_index = u8::try_from(manifest.paks.len())
        .ok()
        .filter(|index| *index < NO_PAK)
        .ok_or_else(|| {
            DomainError::out_of_range(
                "pak count",
                manifest.paks.len() as u64,
                u64::from(NO_PAK - 1),
            )
        })?;

    // ---- Alvos de importação (equivalente a `FileList` + `MatchText`) ----
    let mut targets: Vec<(usize, Vec<u8>)> = Vec::new();
    for file_index in manifest.files_of_pak(pak_index) {
        if let Some(full_path) = manifest.full_name(&manifest.files[file_index])? {
            if Manifest::is_import_target(&full_path) {
                targets.push((file_index, full_path));
            }
        }
    }
    if targets.is_empty() {
        return Err(DomainError::not_found(
            "import target",
            format!(
                "none of the expected targets ({}) exist in pak '{}'",
                IMPORT_TARGETS.join(", "),
                String::from_utf8_lossy(&pak_name)
            ),
        )
        .into());
    }
    if targets.len() < IMPORT_TARGETS.len() {
        ports.progress.report(&ProgressEvent::Info(format!(
            "Warning: only {} of {} import targets were found in this pak",
            targets.len(),
            IMPORT_TARGETS.len()
        )));
    }

    // ---- Novo manifesto: cabeçalho + pastas + lista de paks ----
    let mut manifest_out =
        ManifestWriter::new(BinaryWriter::new(&mut *ports.manifest_out), &manifest);
    manifest_out.write_header()?;
    manifest_out.write_folders()?;
    let pak_count = u32::try_from(manifest.paks.len().saturating_add(1)).map_err(|_| {
        DomainError::out_of_range(
            "pak count",
            manifest.paks.len() as u64,
            u64::from(u32::MAX),
        )
    })?;
    manifest_out.write_pak_count(pak_count)?;
    for pak in &manifest.paks {
        manifest_out.write_pak(pak)?;
    }
    manifest_out.write_pak(&PakEntry {
        unk_byte: NO_PAK,
        name: b"translation".to_vec(),
    })?;

    // ---- Contêineres-alvo -> translation.upak ----
    let mut translation = BinaryWriter::new(&mut *ports.translation);
    let mut pak_reader = BinaryReader::new(&mut *ports.pak);
    let stream_len = pak_reader.stream_len()?;
    let mut new_offsets: HashMap<usize, u64> = HashMap::new();
    let mut report = InsertReport {
        pak_count,
        ..InsertReport::default()
    };

    for (file_index, full_path) in &targets {
        let path_text = String::from_utf8_lossy(full_path).into_owned();
        ports
            .progress
            .report(&ProgressEvent::Info(format!("Importing {path_text}...")));

        let file = &manifest.files[*file_index];
        let obpk = Obpk::parse(&mut pak_reader, file.offset)?;
        if let Some(reason) = obpk.skip_reason() {
            return Err(DomainError::unsupported(format!(
                "cannot import '{path_text}': {reason}"
            ))
            .into());
        }

        let out_base = translation.position()?;

        // Header + tabelas: cópia literal (fiel ao `CopyFrom` do Delphi).
        pak_reader.seek_to(file.offset)?;
        let raw_tables = pak_reader.read_vec(
            obpk.header.data_offset as usize,
            "OBPK header and tables",
        )?;
        translation.write_bytes(&raw_tables)?;

        // Blob original descompactado.
        let blob = read_blob(&mut pak_reader, file.offset, &obpk, stream_len, ports.codec)?;
        let font_container = is_font_container_path(full_path);

        // Remontagem do blob: entradas `.loc`/`.gfx` substituídas pelos
        // arquivos do Dump; os demais payloads são copiados intactos.
        let mut new_blob = Vec::with_capacity(blob.len());
        let mut rebuilt: Vec<RebuiltEntry> = Vec::with_capacity(obpk.names.len());
        let mut cursor = 0usize;
        for entry in &obpk.names {
            let size = entry.size as usize;
            let end = cursor.checked_add(size).ok_or_else(|| {
                DomainError::invalid_data("entry sizes overflow the decompressed blob")
            })?;
            let original = blob.get(cursor..end).ok_or_else(|| {
                DomainError::invalid_data(format!(
                    "entry '{}' exceeds the decompressed blob",
                    String::from_utf8_lossy(&entry.name)
                ))
            })?;

            let replace = font_container || peek_u32(original, 0)? == LOC_MAGIC;
            let replacement: Cow<'_, [u8]> = if !replace {
                Cow::Borrowed(original)
            } else {
                let mut path = full_path.clone();
                path.push(b'\\');
                path.extend_from_slice(&entry.name);
                path.extend_from_slice(if font_container { b".gfx" } else { b".loc" });

                let data = ports
                    .payloads
                    .read_file(&path)
                    .map_err(|source| UseCaseError::adapter("reading a replacement payload", source))?;

                if font_container {
                    // `.gfx` re-empacotado como `u32 tamanho + zlib`.
                    let length = u32::try_from(data.len()).map_err(|_| {
                        DomainError::out_of_range(
                            "gfx payload length",
                            data.len() as u64,
                            u64::from(u32::MAX),
                        )
                    })?;
                    let mut packed = Vec::with_capacity(data.len() + 4);
                    packed.extend_from_slice(&length.to_le_bytes());
                    packed.extend_from_slice(&ports.codec.compress(&data)?);
                    Cow::Owned(packed)
                } else {
                    // `.loc` entra no blob sem compressão (como no legado).
                    Cow::Owned(data)
                }
            };

            let new_size = u32::try_from(replacement.len()).map_err(|_| {
                DomainError::out_of_range(
                    "rebuilt entry length",
                    replacement.len() as u64,
                    u64::from(u32::MAX),
                )
            })?;
            new_blob.extend_from_slice(&replacement);
            rebuilt.push(RebuiltEntry {
                size: new_size,
                replaced: replace,
            });
            cursor = end;
        }

        // Blob recompactado (`u32 tamanho + zlib`).
        let blob_len = u32::try_from(new_blob.len()).map_err(|_| {
            DomainError::out_of_range("blob length", new_blob.len() as u64, u64::from(u32::MAX))
        })?;
        translation.write_u32(blob_len)?;
        translation.write_bytes(&ports.codec.compress(&new_blob)?)?;

        // Segunda tabela + tabela de tamanhos: seeks explícitos para os
        // offsets registrados no parse (fiel ao legado, agora com os
        // valores atualizados do novo blob).
        translation.seek_to(out_base + u64::from(obpk.header.second_table_offset))?;
        let mut source_position = file.offset + u64::from(obpk.header.second_table_offset);
        for (index, original) in obpk.second.iter().enumerate() {
            // Cada entrada: u32 size, u32 unk_data_size, bytes[unk_data_size].
            source_position += 8;
            pak_reader.seek_to(source_position)?;
            let unk_data = pak_reader.read_vec(
                original.unk_data_size as usize,
                "OBPK second table unknown data",
            )?;
            source_position += u64::from(original.unk_data_size);

            let rebuilt_entry = rebuilt.get(index).ok_or_else(|| {
                DomainError::invalid_data("second table and name table differ in length")
            })?;
            let size = if rebuilt_entry.replaced {
                rebuilt_entry.size
            } else {
                original.size
            };
            translation.write_u32(size)?;
            translation.write_u32(original.unk_data_size)?;
            translation.write_bytes(&unk_data)?;
        }

        translation.seek_to(out_base + obpk.name_sizes_offset)?;
        for entry in &rebuilt {
            translation.write_u32(entry.size)?;
        }
        translation.seek_to_end()?;

        new_offsets.insert(*file_index, out_base);
        report.imported += 1;
        ports.progress.report(&ProgressEvent::Imported {
            path: path_text,
            offset: out_base,
        });
    }

    // ---- Tabela de arquivos regravada (alvos apontam para o novo pak) ----
    let file_count = u32::try_from(manifest.files.len()).map_err(|_| {
        DomainError::out_of_range(
            "file count",
            manifest.files.len() as u64,
            u64::from(u32::MAX),
        )
    })?;
    manifest_out.write_file_count(file_count)?;
    for (index, file) in manifest.files.iter().enumerate() {
        let mut entry = file.clone();
        if let Some(&offset) = new_offsets.get(&index) {
            entry.pak_index = new_pak_index;
            entry.offset = offset;
        }
        manifest_out.write_file(&entry)?;
    }
    report.file_count = file_count;

    let tables_end = manifest_out.writer().position()?;
    let tables_size = u32::try_from(
        tables_end
            .checked_sub(ManifestHeader::SIZE)
            .ok_or_else(|| DomainError::invalid_data("tables block smaller than the header"))?,
    )
    .map_err(|_| DomainError::out_of_range("tables block size", tables_end, u64::from(u32::MAX)))?;

    // ---- Demais blocos copiados do manifesto original ----
    let header = &manifest.header;
    let blocks = [
        (header.bod_block_offset, header.bod_block_size),
        (header.null_block_offset, header.null_block_size),
        (header.langs_block_offset, header.langs_block_size),
        (header.last_block_offset, header.last_block_size),
    ];
    let mut new_block_offsets = [0u32; 4];
    for (index, (offset, size)) in blocks.iter().enumerate() {
        manifest_reader.seek_to(u64::from(*offset))?;
        let bytes = manifest_reader.read_vec(*size as usize, "manifest data block")?;
        new_block_offsets[index] = u32::try_from(manifest_out.writer().position()?).map_err(
            |_| DomainError::out_of_range("manifest length", tables_end, u64::from(u32::MAX)),
        )?;
        manifest_out.writer().write_bytes(&bytes)?;
    }

    // ---- Atualização da tabela de blocos no cabeçalho ----
    {
        let writer = manifest_out.writer();
        writer.patch_u32_at(8, new_block_offsets[0])?; // BODBlockOffset
        writer.patch_u32_at(12, new_block_offsets[1])?; // NullBlockOffset
        writer.patch_u32_at(16, new_block_offsets[2])?; // LangsBlockOffset
        writer.patch_u32_at(20, new_block_offsets[3])?; // LastBlockOffset
        writer.patch_u32_at(24, tables_size)?; // TablesBlockSize
    }

    report.translation_size = translation.position()?;
    ports.progress.report(&ProgressEvent::Done {
        summary: format!(
            "{} container(s) imported; translation.upak ({} bytes) and manifest.bin.NEW written",
            report.imported, report.translation_size
        ),
    });
    Ok(report)
}
