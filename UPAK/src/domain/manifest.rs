//! Entidade **MANIFEST.BIN** (índice global dos pacotes `.upak`).
//!
//! Layout derivado do motor Delphi `Vigil_Havok.Types.UPAK`:
//!
//! ```text
//! Header (44 bytes):
//!   u32 magic, u32 tables_block_offset, u32 bod_block_offset,
//!   u32 null_block_offset, u32 langs_block_offset, u32 last_block_offset,
//!   u32 tables_block_size, u32 bod_block_size, u32 null_block_size,
//!   u32 langs_block_size, u32 last_block_size
//! TablesBlock (em `tables_block_offset`):
//!   u32 folders_count, u32 (reservado)
//!   folders[folders_count]:  u64 hash, u32 name_len, utf8[name_len]
//!   u32 paks_count
//!   paks[paks_count]:        u8 unk, u16 name_len, utf8[name_len]
//!   u32 files_count
//!   files[files_count]:      u8 name_len, ansi[name_len], u64 hash,
//!                            u16 unk, u16 folder_index, u8 table_count,
//!                            u16 table[table_count],
//!                            u8 unk, u8 pak_index, u8 unk, u8 unk, u64 offset
//! ```
//!
//! Os nomes são mantidos como bytes crus (`Vec<u8>`) para garantir
//! re-serialização idêntica ao original (fidelidade binária total).

use std::io::{Read, Seek, Write};

use super::binary::{BinaryReader, BinaryWriter};
use super::error::DomainError;

/// Teto defensivo de alocação para tabelas (protege contra contagens corrompidas).
const MAX_TABLE_ENTRIES: u32 = 10_000_000;

/// Índice de "sem pacote" (`$FF` no manifesto).
pub const NO_PAK: u8 = 0xFF;

/// Alvos que a operação de *insert* re-grava em um novo pacote
/// (equivalente a `FileList` do Delphi).
pub const IMPORT_TARGETS: [&str; 2] = [
    "media\\ui\\ui_core",
    "media\\ui\\ui_fonts\\ui_font_default",
];

/// Normaliza separadores e caixa para comparar caminhos do manifesto.
fn normalize_path(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|b| match b {
            b'/' => b'\\',
            b if b.is_ascii_uppercase() => b.to_ascii_lowercase(),
            b => *b,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestHeader {
    pub magic: u32,
    pub tables_block_offset: u32,
    pub bod_block_offset: u32,
    pub null_block_offset: u32,
    pub langs_block_offset: u32,
    pub last_block_offset: u32,
    pub tables_block_size: u32,
    pub bod_block_size: u32,
    pub null_block_size: u32,
    pub langs_block_size: u32,
    pub last_block_size: u32,
}

impl ManifestHeader {
    /// Tamanho fixo do cabeçalho (`0x2C`).
    pub const SIZE: u64 = 44;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub hash: u64,
    pub name: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakEntry {
    pub unk_byte: u8,
    pub name: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: Vec<u8>,
    pub hash: u64,
    pub unk_short: u16,
    pub folder_index: u16,
    pub unk_table: Vec<u16>,
    pub unk_byte1: u8,
    pub pak_index: u8,
    pub unk_byte2: u8,
    pub unk_byte3: u8,
    pub offset: u64,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub header: ManifestHeader,
    pub folders: Vec<FolderEntry>,
    pub paks: Vec<PakEntry>,
    pub files: Vec<FileEntry>,
    /// Campo reservado logo após `folders_count` (re-serializado byte a byte).
    pub tables_reserved: u32,
    /// Offset onde termina a tabela de arquivos (início do bloco BOD).
    pub tables_end: u64,
}

impl Manifest {
    /// Lê o manifesto a partir de qualquer stream seekável.
    pub fn parse<R: Read + Seek>(reader: &mut BinaryReader<R>) -> Result<Self, DomainError> {
        let header = ManifestHeader {
            magic: reader.read_u32("manifest magic")?,
            tables_block_offset: reader.read_u32("manifest tables block offset")?,
            bod_block_offset: reader.read_u32("manifest BOD block offset")?,
            null_block_offset: reader.read_u32("manifest null block offset")?,
            langs_block_offset: reader.read_u32("manifest langs block offset")?,
            last_block_offset: reader.read_u32("manifest last block offset")?,
            tables_block_size: reader.read_u32("manifest tables block size")?,
            bod_block_size: reader.read_u32("manifest BOD block size")?,
            null_block_size: reader.read_u32("manifest null block size")?,
            langs_block_size: reader.read_u32("manifest langs block size")?,
            last_block_size: reader.read_u32("manifest last block size")?,
        };

        reader.seek_to(u64::from(header.tables_block_offset))?;

        let folders_count = reader.read_u32("manifest folders count")?;
        if folders_count > MAX_TABLE_ENTRIES {
            return Err(DomainError::out_of_range(
                "manifest folders count",
                u64::from(folders_count),
                u64::from(MAX_TABLE_ENTRIES),
            ));
        }
        // Campo reservado (Delphi: `Seek(4, soCurrent)`), preservado para
        // re-serialização idêntica.
        let tables_reserved = reader.read_u32("manifest tables reserved field")?;

        let mut folders = Vec::with_capacity(folders_count as usize);
        for _ in 0..folders_count {
            let hash = reader.read_u64("folder hash")?;
            let name_len = reader.read_u32("folder name length")? as usize;
            let name = reader.read_vec(name_len, "folder name")?;
            folders.push(FolderEntry { hash, name });
        }

        let pak_count = reader.read_u32("manifest pak count")?;
        if pak_count > MAX_TABLE_ENTRIES {
            return Err(DomainError::out_of_range(
                "manifest pak count",
                u64::from(pak_count),
                u64::from(MAX_TABLE_ENTRIES),
            ));
        }

        let mut paks = Vec::with_capacity(pak_count as usize);
        for _ in 0..pak_count {
            let unk_byte = reader.read_u8("pak unknown byte")?;
            let name_len = reader.read_u16("pak name length")? as usize;
            let name = reader.read_vec(name_len, "pak name")?;
            paks.push(PakEntry { unk_byte, name });
        }

        let file_count = reader.read_u32("manifest file count")?;
        if file_count > MAX_TABLE_ENTRIES {
            return Err(DomainError::out_of_range(
                "manifest file count",
                u64::from(file_count),
                u64::from(MAX_TABLE_ENTRIES),
            ));
        }

        let mut files = Vec::with_capacity(file_count as usize);
        for _ in 0..file_count {
            let name_len = reader.read_u8("file name length")? as usize;
            let name = reader.read_vec(name_len, "file name")?;
            let hash = reader.read_u64("file hash")?;
            let unk_short = reader.read_u16("file unknown short")?;
            let folder_index = reader.read_u16("file folder index")?;
            let table_count = reader.read_u8("file unknown table count")?;
            let mut unk_table = Vec::with_capacity(table_count as usize);
            for _ in 0..table_count {
                unk_table.push(reader.read_u16("file unknown table entry")?);
            }
            let unk_byte1 = reader.read_u8("file unknown byte 1")?;
            let pak_index = reader.read_u8("file pak index")?;
            let unk_byte2 = reader.read_u8("file unknown byte 2")?;
            let unk_byte3 = reader.read_u8("file unknown byte 3")?;
            let offset = reader.read_u64("file offset")?;
            files.push(FileEntry {
                name,
                hash,
                unk_short,
                folder_index,
                unk_table,
                unk_byte1,
                pak_index,
                unk_byte2,
                unk_byte3,
                offset,
            });
        }

        let tables_end = reader.position()?;

        Ok(Self {
            header,
            folders,
            paks,
            files,
            tables_reserved,
            tables_end,
        })
    }

    /// Caminho completo (`pak\pasta\arquivo`) de uma entrada, ou `None`
    /// quando a entrada não pertence a nenhum pak (`$FF`).
    ///
    /// Retorna erro quando índices de pak/pasta estão fora dos limites
    /// (o Delphi estouraria em tempo de execução).
    pub fn full_name(&self, file: &FileEntry) -> Result<Option<Vec<u8>>, DomainError> {
        if file.pak_index == NO_PAK {
            return Ok(None);
        }
        let pak = self.paks.get(usize::from(file.pak_index)).ok_or_else(|| {
            DomainError::out_of_range(
                "file pak index",
                u64::from(file.pak_index),
                self.paks.len() as u64,
            )
        })?;
        let folder = self.folders.get(usize::from(file.folder_index)).ok_or_else(|| {
            DomainError::out_of_range(
                "file folder index",
                u64::from(file.folder_index),
                self.folders.len() as u64,
            )
        })?;

        let mut path = Vec::with_capacity(
            pak.name.len() + folder.name.len() + file.name.len() + 2,
        );
        path.extend_from_slice(&pak.name);
        path.push(b'\\');
        if !folder.name.is_empty() {
            path.extend_from_slice(&folder.name);
            path.push(b'\\');
        }
        path.extend_from_slice(&file.name);
        Ok(Some(path))
    }

    /// Índice do pak cujo nome é exatamente `stem` (comparação por bytes,
    /// como no Delphi `Paks[I].Name = PakName`).
    pub fn pak_index_of(&self, stem: &[u8]) -> Option<usize> {
        self.paks.iter().position(|pak| pak.name == stem)
    }

    /// Índices (na ordem do manifesto) dos arquivos de um pak.
    pub fn files_of_pak(&self, pak_index: usize) -> Vec<usize> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, file)| usize::from(file.pak_index) == pak_index)
            .map(|(index, _)| index)
            .collect()
    }

    /// Verifica se o caminho completo é um dos alvos de *insert*
    /// (equivalente a `MatchText`, que no Delphi compara sem diferenciar caixa).
    pub fn is_import_target(full_path: &[u8]) -> bool {
        let normalized = normalize_path(full_path);
        IMPORT_TARGETS
            .iter()
            .any(|target| normalized == target.as_bytes())
    }
}

/// Serializador do manifesto (usado na montagem do `manifest.bin.NEW`).
///
/// Reproduz fielmente o layout lido por [`Manifest::parse`].
pub struct ManifestWriter<'a, W: Write + Seek> {
    writer: BinaryWriter<W>,
    manifest: &'a Manifest,
}

impl<'a, W: Write + Seek> ManifestWriter<'a, W> {
    pub fn new(writer: BinaryWriter<W>, manifest: &'a Manifest) -> Self {
        Self { writer, manifest }
    }

    pub fn writer(&mut self) -> &mut BinaryWriter<W> {
        &mut self.writer
    }

    /// Cabeçalho (44 bytes) preservando os offsets originais.
    pub fn write_header(&mut self) -> Result<(), DomainError> {
        let h = &self.manifest.header;
        self.writer.write_u32(h.magic)?;
        self.writer.write_u32(h.tables_block_offset)?;
        self.writer.write_u32(h.bod_block_offset)?;
        self.writer.write_u32(h.null_block_offset)?;
        self.writer.write_u32(h.langs_block_offset)?;
        self.writer.write_u32(h.last_block_offset)?;
        self.writer.write_u32(h.tables_block_size)?;
        self.writer.write_u32(h.bod_block_size)?;
        self.writer.write_u32(h.null_block_size)?;
        self.writer.write_u32(h.langs_block_size)?;
        self.writer.write_u32(h.last_block_size)?;
        Ok(())
    }

    pub fn write_folders(&mut self) -> Result<(), DomainError> {
        let count = u32::try_from(self.manifest.folders.len()).map_err(|_| {
            DomainError::out_of_range("folder count", self.manifest.folders.len() as u64, u64::from(u32::MAX))
        })?;
        self.writer.write_u32(count)?;
        self.writer.write_u32(self.manifest.tables_reserved)?;
        for folder in &self.manifest.folders {
            self.writer.write_u64(folder.hash)?;
            let len = u32::try_from(folder.name.len()).map_err(|_| {
                DomainError::out_of_range("folder name length", folder.name.len() as u64, u64::from(u32::MAX))
            })?;
            self.writer.write_u32(len)?;
            self.writer.write_bytes(&folder.name)?;
        }
        Ok(())
    }

    pub fn write_pak_count(&mut self, count: u32) -> Result<(), DomainError> {
        self.writer.write_u32(count)
    }

    pub fn write_pak(&mut self, pak: &PakEntry) -> Result<(), DomainError> {
        self.writer.write_u8(pak.unk_byte)?;
        let len = u16::try_from(pak.name.len()).map_err(|_| {
            DomainError::out_of_range("pak name length", pak.name.len() as u64, u64::from(u16::MAX))
        })?;
        self.writer.write_u16(len)?;
        self.writer.write_bytes(&pak.name)?;
        Ok(())
    }

    pub fn write_file_count(&mut self, count: u32) -> Result<(), DomainError> {
        self.writer.write_u32(count)
    }

    pub fn write_file(&mut self, file: &FileEntry) -> Result<(), DomainError> {
        let name_len = u8::try_from(file.name.len()).map_err(|_| {
            DomainError::out_of_range("file name length", file.name.len() as u64, u64::from(u8::MAX))
        })?;
        self.writer.write_u8(name_len)?;
        self.writer.write_bytes(&file.name)?;
        self.writer.write_u64(file.hash)?;
        self.writer.write_u16(file.unk_short)?;
        self.writer.write_u16(file.folder_index)?;
        let table_len = u8::try_from(file.unk_table.len()).map_err(|_| {
            DomainError::out_of_range("file unknown table length", file.unk_table.len() as u64, u64::from(u8::MAX))
        })?;
        self.writer.write_u8(table_len)?;
        for value in &file.unk_table {
            self.writer.write_u16(*value)?;
        }
        self.writer.write_u8(file.unk_byte1)?;
        self.writer.write_u8(file.pak_index)?;
        self.writer.write_u8(file.unk_byte2)?;
        self.writer.write_u8(file.unk_byte3)?;
        self.writer.write_u64(file.offset)?;
        Ok(())
    }
}
