//! Entidade **OBPK** — contêiner embarcado dentro de um arquivo `.upak`.
//!
//! Cada entrada da tabela de arquivos do manifesto aponta para um OBPK
//! (offset dentro do `.upak`). Layout derivado de `Vigil_Havok.Types.OBPK`:
//!
//! ```text
//! Header (30 bytes, packed):
//!   [4] "OBPK", u8 null, u32 unknown, u32 first_table_offset,
//!   u32 first_table_size, u32 second_table_offset, u32 second_table_size,
//!   u32 data_offset, u8 chunked
//! FirstTable (base + first_table_offset):
//!   u32 unk_entry_count, u32 file_count, u32 size, u32 (reservado),
//!   bytes[size], u32 e, u32 g, u32 h, u32 j,
//!   entries[unk_entry_count]: u32 k, u32 byte_count, u32 m, u32 n,
//!                             bytes[byte_count]
//!   u8 has_names -> se verdadeiro: u32 unk, então por arquivo:
//!                   u64 hash, u32 name_len, ansi[name_len]
//! SecondTable (base + second_table_offset):
//!   por arquivo: u32 size, u32 unk_data_size, bytes[unk_data_size]
//!   seguido de:  u32 size_table[file_count]  (tamanho de cada payload)
//! Payload (base + data_offset):
//!   u32 uncompressed_size, zlib(...)
//! ```
//!
//! Os payloads são *cumulativos* dentro do blob: a entrada `i` ocupa
//! `size_table[i]` bytes consecutivos.

use std::io::{Read, Seek};

use super::binary::BinaryReader;
use super::error::DomainError;

pub const OBPK_MAGIC: [u8; 4] = *b"OBPK";

/// Magic `LOC\0x02` exigido pelo formato de localização.
pub const LOC_MAGIC: u32 = 0x0243_4F4C; // 37965644

/// Trecho de caminho que identifica o contêiner de fontes (`.gfx`).
const FONT_FOLDER_HINT: &str = "ui\\ui_fonts\\ui_font_default";

const MAX_TABLE_ENTRIES: u32 = 10_000_000;
const MAX_NAME_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObpkHeader {
    pub magic: [u8; 4],
    pub null_terminator: u8,
    pub unknown: u32,
    pub first_table_offset: u32,
    pub first_table_size: u32,
    pub second_table_offset: u32,
    pub second_table_size: u32,
    pub data_offset: u32,
    pub chunked: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstTableEntry {
    pub k: u32,
    pub byte_count: u32,
    pub m: u32,
    pub n: u32,
}

/// Primeira tabela do contêiner.
///
/// Todos os campos são lidos durante o *parse* para representar fielmente o
/// formato (e alimentar as validações), mas o Insert copia a tabela
/// literalmente do arquivo original — por isso alguns valores não são
/// consumidos diretamente pelos casos de uso.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FirstTable {
    pub unk_entry_count: u32,
    pub file_count: u32,
    pub size: u32,
    pub reserved: u32,
    pub e: u32,
    pub g: u32,
    pub h: u32,
    pub j: u32,
    pub entries: Vec<FirstTableEntry>,
    pub has_names: bool,
    /// Presente apenas quando `has_names` é verdadeiro.
    pub unk_has_names: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameEntry {
    pub hash: u64,
    /// Nome em bytes crus (ANSI). Sem nomes no arquivo, é o índice decimal.
    pub name: Vec<u8>,
    /// Tamanho do payload da entrada dentro do blob (`size_table`).
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondTableEntry {
    pub size: u32,
    pub unk_data_size: u32,
}

#[derive(Debug, Clone)]
pub struct Obpk {
    pub header: ObpkHeader,
    pub first_table: FirstTable,
    pub names: Vec<NameEntry>,
    pub second: Vec<SecondTableEntry>,
    /// Offset (relativo à base do contêiner) logo após as entradas da segunda
    /// tabela — onde começa a tabela de tamanhos `name_sizes`.
    pub name_sizes_offset: u64,
}

/// O que fazer com o payload de uma entrada durante o *dump*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    /// Arquivo `.loc` (magic `LOC\0x02`) — é extraído.
    Loc,
    /// Arquivo `.gfx` empacotado como `u32 tamanho + zlib` — é extraído.
    Gfx,
    /// Demais formatos (DDS, BOD, Wwise...) — ignorados pelo *dump*.
    Skip,
}

/// Plano de extração de uma entrada do blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPlan {
    /// Índice da entrada no contêiner.
    pub index: usize,
    pub name: Vec<u8>,
    /// Offset do payload dentro do blob descomprimido.
    pub offset: usize,
    pub size: usize,
    pub kind: PayloadKind,
}

impl Obpk {
    /// Lê a estrutura OBPK que inicia em `base_offset` do stream.
    pub fn parse<R: Read + Seek>(
        reader: &mut BinaryReader<R>,
        base_offset: u64,
    ) -> Result<Self, DomainError> {
        reader.seek_to(base_offset)?;

        let magic = reader.read_array::<4>("OBPK magic")?;
        if magic != OBPK_MAGIC {
            return Err(DomainError::invalid_magic(
                "OBPK container",
                String::from_utf8_lossy(&OBPK_MAGIC).into_owned(),
                format!("0x{:02X?}", magic),
            ));
        }

        let header = ObpkHeader {
            magic,
            null_terminator: reader.read_u8("OBPK null terminator")?,
            unknown: reader.read_u32("OBPK unknown field")?,
            first_table_offset: reader.read_u32("OBPK first table offset")?,
            first_table_size: reader.read_u32("OBPK first table size")?,
            second_table_offset: reader.read_u32("OBPK second table offset")?,
            second_table_size: reader.read_u32("OBPK second table size")?,
            data_offset: reader.read_u32("OBPK data offset")?,
            chunked: reader.read_u8("OBPK chunked flag")?,
        };

        // ---- Primeira tabela ----
        reader.seek_to(base_offset + u64::from(header.first_table_offset))?;
        let unk_entry_count = reader.read_u32("OBPK first table entry count")?;
        let file_count = reader.read_u32("OBPK file count")?;
        if unk_entry_count > MAX_TABLE_ENTRIES || file_count > MAX_TABLE_ENTRIES {
            return Err(DomainError::out_of_range(
                "OBPK table counts",
                u64::from(unk_entry_count.max(file_count)),
                u64::from(MAX_TABLE_ENTRIES),
            ));
        }
        let size = reader.read_u32("OBPK first table size field")?;
        let reserved = reader.read_u32("OBPK first table reserved")?;
        reader.skip(u64::from(size))?;

        let e = reader.read_u32("OBPK first table field e")?;
        let g = reader.read_u32("OBPK first table field g")?;
        let h = reader.read_u32("OBPK first table field h")?;
        let j = reader.read_u32("OBPK first table field j")?;

        let mut entries = Vec::with_capacity(unk_entry_count as usize);
        for _ in 0..unk_entry_count {
            let entry = FirstTableEntry {
                k: reader.read_u32("OBPK entry k")?,
                byte_count: reader.read_u32("OBPK entry byte count")?,
                m: reader.read_u32("OBPK entry m")?,
                n: reader.read_u32("OBPK entry n")?,
            };
            reader.skip(u64::from(entry.byte_count))?;
            entries.push(entry);
        }

        let has_names = reader.read_u8("OBPK has names flag")? != 0;
        let mut names: Vec<NameEntry> = Vec::with_capacity(file_count as usize);
        let unk_has_names = if has_names {
            let unk = reader.read_u32("OBPK unknown has-names field")?;
            for _ in 0..file_count {
                let hash = reader.read_u64("OBPK name hash")?;
                let name_len = reader.read_u32("OBPK name length")? as usize;
                if name_len > MAX_NAME_BYTES {
                    return Err(DomainError::out_of_range(
                        "OBPK name length",
                        name_len as u64,
                        MAX_NAME_BYTES as u64,
                    ));
                }
                let name = reader.read_vec(name_len, "OBPK file name")?;
                names.push(NameEntry { hash, name, size: 0 });
            }
            Some(unk)
        } else {
            // Sem tabela de nomes, o Delphi usa o índice como nome.
            for index in 0..file_count {
                names.push(NameEntry {
                    hash: 0,
                    name: index.to_string().into_bytes(),
                    size: 0,
                });
            }
            None
        };

        let first_table = FirstTable {
            unk_entry_count,
            file_count,
            size,
            reserved,
            e,
            g,
            h,
            j,
            entries,
            has_names,
            unk_has_names,
        };

        // ---- Segunda tabela + tamanhos ----
        reader.seek_to(base_offset + u64::from(header.second_table_offset))?;
        let mut second = Vec::with_capacity(file_count as usize);
        for _ in 0..file_count {
            let entry = SecondTableEntry {
                size: reader.read_u32("OBPK second table size")?,
                unk_data_size: reader.read_u32("OBPK second table unknown data size")?,
            };
            reader.skip(u64::from(entry.unk_data_size))?;
            second.push(entry);
        }
        // Tabela de tamanhos (posições 0..file_count logo após as entradas).
        let name_sizes_position = reader.position()?;
        for entry in names.iter_mut() {
            entry.size = reader.read_u32("OBPK payload size")?;
        }

        Ok(Self {
            header,
            first_table,
            names,
            second,
            name_sizes_offset: name_sizes_position - base_offset,
        })
    }

    /// Motivo pelo qual um contêiner é ignorado, se for o caso.
    pub fn skip_reason(&self) -> Option<&'static str> {
        if self.header.chunked != 0 {
            Some("chunked container")
        } else if self.first_table.file_count == 0 {
            Some("empty container")
        } else if self.header.second_table_size == 0 {
            Some("container without data block")
        } else {
            None
        }
    }

    /// Calcula o plano de extração das entradas do blob descomprimido.
    ///
    /// * `blob` — blob descomprimido na íntegra (valida tamanhos e magics);
    /// * `folder_path` — caminho do contêiner com `\` (ex.: `media\ui\ui_core`),
    ///   usado para detectar o contêiner de fontes (`.gfx`).
    pub fn plan_extraction(
        &self,
        blob: &[u8],
        folder_path: &[u8],
    ) -> Result<Vec<EntryPlan>, DomainError> {
        let is_font_container = is_font_container_path(folder_path);

        let mut plans = Vec::with_capacity(self.names.len());
        let mut offset: usize = 0;
        for (index, entry) in self.names.iter().enumerate() {
            let size = entry.size as usize;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| DomainError::invalid_data("entry offsets overflow the blob"))?;
            if end > blob.len() {
                return Err(DomainError::invalid_data(format!(
                    "entry {} ({}) exceeds the decompressed blob ({} > {})",
                    index,
                    String::from_utf8_lossy(&entry.name),
                    end,
                    blob.len()
                )));
            }

            let kind = if is_font_container {
                PayloadKind::Gfx
            } else if peek_u32(blob, offset)? == LOC_MAGIC {
                PayloadKind::Loc
            } else {
                PayloadKind::Skip
            };

            plans.push(EntryPlan {
                index,
                name: entry.name.clone(),
                offset,
                size,
                kind,
            });
            offset = end;
        }
        Ok(plans)
    }
}

/// O caminho corresponde ao contêiner de fontes (`.gfx`), cujos payloads
/// são exportados/importados no formato `u32 tamanho + zlib`.
pub fn is_font_container_path(folder_path: &[u8]) -> bool {
    contains_ignore_ascii_case(folder_path, FONT_FOLDER_HINT.as_bytes())
}

/// Lê um `u32` little-endian em `offset`, sem panic: erro tipado se o blob
/// não tiver 4 bytes a partir da posição.
pub fn peek_u32(blob: &[u8], offset: usize) -> Result<u32, DomainError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| DomainError::invalid_data("payload offset overflow"))?;
    let bytes = blob
        .get(offset..end)
        .ok_or_else(|| DomainError::invalid_data("payload ends before a magic number could be read"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Procura `needle` em `haystack` ignorando caixa ASCII (separadores já `\`).
fn contains_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}
