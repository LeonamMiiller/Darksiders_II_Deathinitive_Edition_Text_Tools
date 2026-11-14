//! Entidade [`LocFile`] — formato `.loc` da Darksiders II Deathinitive
//! Edition (tabela de IDs + tabela de textos + bloco de textos + bloco Z).
//!
//! Fiel ao `Vigil_Havok.Types.LOC.pas` do Delphi:
//!
//! * o **parse** lê o cabeçalho do arquivo, o bloco da primeira língua e o
//!   resto do arquivo através de `Read + Seek`, validando magics e faixas;
//! * o **export** percorre as referências na ordem `tabela de textos` ->
//!   `tabela de IDs`, deduplica em primeira ocorrência e aplica
//!   [`break_line`] (port de `Delutto.Utils.BreakLine`);
//! * o **import** regrava o bloco da língua com os novos textos, atualiza as
//!   duas tabelas, `TextBlockSize` e `LangBlockSize` — e também
//!   `ZBlockOffset` (desvio documentado: o Delphi calculava o valor mas
//!   nunca o gravava, deixando o arquivo apontando para o bloco Z antigo).
//!
//! Os textos são mantidos como **bytes crus** (`Vec<u8>`, UTF-8 por
//! convenção) para permitir re-serialização byte-idêntica.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Seek};

use super::binary::{BinaryReader, BinaryWriter};
use super::error::DomainError;
use super::text::{break_line, join_lines, split_lines};

/// `LOC\x02` — magic do arquivo (`37965644`).
pub const LOC_MAGIC: u32 = 0x0243_4F4C;

/// Magic do bloco da língua (`2`).
const BLOCK_MAGIC: u64 = 2;

/// Tamanho do cabeçalho do bloco da língua.
const BLOCK_HEADER_SIZE: usize = 56;

/// Tamanho de uma entrada da tabela de textos (`u32 offset` + 12 bytes).
const TEXT_TABLE_ENTRY: usize = 16;

/// Tamanho de uma entrada da tabela de IDs (offset de texto em `u64` no +8).
const ID_TABLE_ENTRY: usize = 40;

/// Deslocamento do `u64` de offset dentro de uma entrada da tabela de IDs.
const ID_TEXT_OFFSET: usize = 8;

/// Arquivo `.loc` carregado: cabeçalho, bloco da língua e resto preservados
/// byte a byte, mais as tabelas já resolvidas em [`LocFile::offsets`] /
/// [`LocFile::texts`].
#[derive(Debug, Clone)]
pub struct LocFile {
    /// Bytes `[0, lang_off)` — cabeçalho do arquivo + índice de línguas.
    head: Vec<u8>,
    /// Bloco da primeira língua (o único reescrito pelo Import).
    block: Vec<u8>,
    /// Bytes após o bloco da língua (demais línguas/blocos: preservados).
    tail: Vec<u8>,
    /// Offset do bloco da língua dentro do arquivo.
    lang_off: usize,
    /// `TextBlockSize` original do bloco.
    text_block_size: u32,
    /// `ZBlockSize` do bloco.
    z_block_size: u32,
    /// Offset da tabela de IDs dentro do bloco.
    id_table_off: usize,
    /// Offset da tabela de textos dentro do bloco.
    text_table_off: usize,
    /// Offset do bloco de textos dentro do bloco.
    text_block_off: usize,
    /// Offset do bloco Z dentro do bloco.
    z_block_off: usize,
    /// Offsets de texto na ordem do legado: primeiro a tabela de textos,
    /// depois a tabela de IDs (truncado para `u32`, como no Delphi).
    offsets: Vec<u32>,
    /// Textos correspondentes (bytes crus, sem o `NUL` final).
    texts: Vec<Vec<u8>>,
    /// Entradas da tabela de textos.
    text_count: u32,
}

/// Resultado do **export**.
#[derive(Debug, Clone)]
pub struct ExportOutcome {
    /// TXT completo (BOM UTF-8 + linhas separadas por CRLF, sem CRLF final).
    pub txt: Vec<u8>,
    /// Total de referências de texto (tabela de textos + tabela de IDs).
    pub texts: usize,
    /// Textos únicos gravados no TXT.
    pub unique: usize,
}

/// Estatísticas do **import**.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    /// Linhas lidas do TXT.
    pub lines: usize,
    /// Textos únicos regravados.
    pub unique: usize,
    /// Linhas do TXT que sobraram (ignoradas, como no legado).
    pub extra: usize,
    /// `TextBlockSize` anterior.
    pub old_text_block_size: u32,
    /// `TextBlockSize` novo.
    pub new_text_block_size: u32,
    /// `LangBlockSize` novo.
    pub new_lang_block_size: u32,
}

/// Resultado do **import**: arquivo completo + estatísticas.
#[derive(Debug, Clone)]
pub struct ImportOutcome {
    /// Novo conteúdo do `.loc`, pronto para gravação.
    pub data: Vec<u8>,
    /// O que mudou.
    pub stats: ImportStats,
}

impl LocFile {
    /// Lê e valida um `.loc` a partir de qualquer `Read + Seek`.
    ///
    /// Espelha o construtor `TLoc.Create` do Delphi: magics conferidos,
    /// índice de línguas calculado como `LangCount * 8 + 8` e o bloco da
    /// primeira língua carregado por inteiro (o resto do arquivo também é
    /// mantido, para que o Import possa regravá-lo byte a byte).
    pub fn parse<R: Read + Seek>(reader: R) -> Result<Self, DomainError> {
        let mut reader = BinaryReader::new(reader);
        let total = reader.stream_len()?;

        reader.seek_to(0)?;
        let magic = reader.read_u32("LOC file magic")?;
        if magic != LOC_MAGIC {
            return Err(DomainError::invalid_magic(
                "LOC file",
                format!("0x{LOC_MAGIC:08X}"),
                format!("0x{magic:08X}"),
            ));
        }
        let lang_count = reader.read_u32("LOC language count")?;
        if lang_count == 0 {
            return Err(DomainError::invalid_data(
                "the LOC file declares no languages",
            ));
        }
        let lang_off = usize::try_from(lang_count)
            .ok()
            .and_then(|count| count.checked_mul(8))
            .and_then(|bytes| bytes.checked_add(8))
            .ok_or_else(|| DomainError::invalid_data("language count overflows the header"))?;
        if lang_off as u64 > total {
            return Err(DomainError::out_of_range(
                "language index",
                lang_off as u64,
                total,
            ));
        }

        // Índice da primeira língua: `LangBlockIndex` (ignorado) + tamanho.
        reader.seek_to(8)?;
        let _lang_block_index = reader.read_u32("LOC language block index")?;
        let lang_block_size = reader.read_u32("LOC language block size")?;
        let lang_block_end = (lang_off as u64)
            .checked_add(u64::from(lang_block_size))
            .ok_or_else(|| DomainError::invalid_data("language block size overflows"))?;
        if lang_block_end > total {
            return Err(DomainError::out_of_range(
                "language block",
                lang_block_end,
                total,
            ));
        }

        // Cabeçalho + bloco + resto (como o `CopyFrom`/`LoadFromFile` legado).
        reader.seek_to(0)?;
        let head = reader.read_vec(lang_off, "LOC header")?;
        let block = reader.read_vec(lang_block_size as usize, "LOC language block")?;
        let tail_len = usize::try_from(total - lang_block_end)
            .map_err(|_| DomainError::out_of_range("LOC length", total, usize::MAX as u64))?;
        let tail = reader.read_vec(tail_len, "LOC trailing data")?;

        Self::from_parts(head, block, tail, lang_off)
    }

    /// Monta a entidade a partir das três fatias já lidas (usado pelo parse
    /// e pelos testes, que constroem arquivos sintéticos).
    fn from_parts(
        head: Vec<u8>,
        block: Vec<u8>,
        tail: Vec<u8>,
        lang_off: usize,
    ) -> Result<Self, DomainError> {
        if block.len() < BLOCK_HEADER_SIZE {
            return Err(DomainError::invalid_data(format!(
                "the language block is too small: {} byte(s), at least {BLOCK_HEADER_SIZE} expected",
                block.len()
            )));
        }

        let block_magic = u64_at(&block, 0, "language block magic")?;
        if block_magic != BLOCK_MAGIC {
            return Err(DomainError::invalid_magic(
                "LOC language block",
                BLOCK_MAGIC.to_string(),
                block_magic.to_string(),
            ));
        }

        let id_count = u32_at(&block, 8, "ID count")?;
        let text_count = u32_at(&block, 12, "text count")?;
        let text_block_size = u32_at(&block, 16, "text block size")?;
        let z_block_size = u32_at(&block, 20, "Z block size")?;
        let id_table_off = to_usize(u64_at(&block, 24, "ID table offset")?, "ID table offset")?;
        let text_table_off =
            to_usize(u64_at(&block, 32, "text table offset")?, "text table offset")?;
        let text_block_off =
            to_usize(u64_at(&block, 40, "text block offset")?, "text block offset")?;
        let z_block_off = to_usize(u64_at(&block, 48, "Z block offset")?, "Z block offset")?;

        // Faixas das tabelas (o Delphi falharia ao buscar fora do bloco).
        let text_table_len = count_len(text_count as usize, TEXT_TABLE_ENTRY, "text table")?;
        require_range(&block, text_table_off, text_table_len, "text table")?;
        let id_table_len = count_len(id_count as usize, ID_TABLE_ENTRY, "ID table")?;
        require_range(&block, id_table_off, id_table_len, "ID table")?;
        if text_block_off > block.len() {
            return Err(DomainError::out_of_range(
                "text block offset",
                text_block_off as u64,
                block.len() as u64,
            ));
        }
        if z_block_off > block.len() {
            return Err(DomainError::out_of_range(
                "Z block offset",
                z_block_off as u64,
                block.len() as u64,
            ));
        }

        // Tabela de textos: `u32 offset` por entrada.
        let mut offsets = Vec::with_capacity(text_count as usize + id_count as usize);
        for index in 0..text_count as usize {
            let entry = text_table_off + index * TEXT_TABLE_ENTRY;
            offsets.push(u32_at(&block, entry, "text table entry")?);
        }
        // Tabela de IDs: `u64` em +8, truncado para `u32` (como no Delphi,
        // que lia `UInt64` em um campo `UInt32`).
        for index in 0..id_count as usize {
            let entry = id_table_off + index * ID_TABLE_ENTRY;
            let raw = u64_at(&block, entry + ID_TEXT_OFFSET, "ID table entry")?;
            offsets.push(raw as u32);
        }

        let mut texts = Vec::with_capacity(offsets.len());
        for &offset in &offsets {
            texts.push(c_str_at(&block, text_block_off, offset).to_vec());
        }

        Ok(Self {
            head,
            block,
            tail,
            lang_off,
            text_block_size,
            z_block_size,
            id_table_off,
            text_table_off,
            text_block_off,
            z_block_off,
            offsets,
            texts,
            text_count,
        })
    }

    /// Total de referências de texto (tabela de textos + tabela de IDs).
    pub fn text_references(&self) -> usize {
        self.texts.len()
    }

    /// Exporta os textos únicos (em primeira ocorrência) para o formato TXT
    /// usado pelos tradutores: BOM UTF-8 + CRLF, com `BreakLine` aplicado.
    pub fn export_txt(&self) -> ExportOutcome {
        let mut seen: HashSet<&[u8]> = HashSet::new();
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for text in &self.texts {
            if seen.insert(text.as_slice()) {
                lines.push(break_line(text));
            }
        }
        let unique = lines.len();
        ExportOutcome {
            txt: join_lines(&lines),
            texts: self.texts.len(),
            unique,
        }
    }

    /// Importa as linhas de um TXT, regravando o bloco da língua.
    ///
    /// Cada texto único consome uma linha (na mesma ordem do export); os
    /// textos repetidos reutilizam o offset já gravado. Ao final:
    ///
    /// * `TextBlockSize` (bloco 16) e as duas tabelas são atualizados;
    /// * `ZBlockOffset` (bloco 48) recebe o valor **novo** — desvio
    ///   documentado em relação ao Delphi, que não o gravava;
    /// * `LangBlockSize` (offset 12 do arquivo) é regravado.
    ///
    /// Linhas que sobrarem no TXT são ignoradas (como no legado); faltar
    /// linha é erro.
    pub fn import_txt(&self, txt: &[u8]) -> Result<ImportOutcome, DomainError> {
        let lines = split_lines(txt);

        // Header + tabelas preservados; os textos são reescritos a partir
        // do fim das tabelas (o bloco legado descartava os ~215 KB de
        // estrutura não referenciada, idem ao Delphi).
        let mut new_block = Vec::with_capacity(self.block.len());
        new_block.extend_from_slice(&self.block[..self.text_block_off]);

        let mut seen: HashMap<&[u8], u32> = HashMap::new();
        let mut new_offsets: Vec<u32> = Vec::with_capacity(self.offsets.len());
        let mut consumed = 0usize;

        for &offset in &self.offsets {
            let original = self.text_at(offset);
            if let Some(&existing) = seen.get(original) {
                new_offsets.push(existing);
                continue;
            }
            let line = lines.get(consumed).ok_or_else(|| {
                DomainError::not_found(
                    "TXT line",
                    format!(
                        "the TXT ends after {} line(s) but the LOC file requires more unique texts",
                        lines.len()
                    ),
                )
            })?;
            consumed += 1;

            let relative = new_block.len() - self.text_block_off;
            let relative = u32::try_from(relative).map_err(|_| {
                DomainError::out_of_range(
                    "text block size",
                    (new_block.len() - self.text_block_off) as u64,
                    u64::from(u32::MAX),
                )
            })?;
            new_block.extend_from_slice(&break_line(line));
            new_block.push(0);
            seen.insert(original, relative);
            new_offsets.push(relative);
        }

        let unique = consumed;
        let extra = lines.len().saturating_sub(unique);
        let new_text_block_size = u32::try_from(new_block.len() - self.text_block_off).map_err(
            |_| {
                DomainError::out_of_range(
                    "text block size",
                    (new_block.len() - self.text_block_off) as u64,
                    u64::from(u32::MAX),
                )
            },
        )?;

        // Offset do novo bloco Z: capturado antes de copiá-lo (o texto
        // recém-gravado termina exatamente aí).
        let z_block_off_new = new_block.len();
        // Bloco Z, quando existe, vai logo após os textos novos.
        if self.z_block_size > 0 {
            let z_end = self
                .z_block_off
                .checked_add(self.z_block_size as usize)
                .ok_or_else(|| DomainError::invalid_data("Z block range overflows"))?;
            let z_block = self.block.get(self.z_block_off..z_end).ok_or_else(|| {
                DomainError::invalid_data(format!(
                    "the Z block [{}, {}) exceeds the language block of {} byte(s)",
                    self.z_block_off,
                    z_end,
                    self.block.len()
                ))
            })?;
            new_block.extend_from_slice(z_block);
        }
        let new_lang_block_size = u32::try_from(new_block.len()).map_err(|_| {
            DomainError::out_of_range("language block size", new_block.len() as u64, u64::from(u32::MAX))
        })?;

        // Montagem final: cabeçalho original + bloco novo + resto original.
        let mut data = Vec::with_capacity(self.head.len() + new_block.len() + self.tail.len());
        data.extend_from_slice(&self.head);
        data.extend_from_slice(&new_block);
        data.extend_from_slice(&self.tail);

        let lang_off = self.lang_off as u64;
        let data = {
            let mut cursor = Cursor::new(data);
            {
                let mut writer = BinaryWriter::new(&mut cursor);

                // `LangBlockSize` (offset 12 do arquivo).
                writer.patch_u32_at(12, new_lang_block_size)?;
                // `TextBlockSize` (bloco 16) — fiel ao `NewData.Position := 16`.
                writer.patch_u32_at(lang_off + 16, new_text_block_size)?;
                // `ZBlockOffset` (bloco 48): desvio documentado do legado.
                writer.patch_u64_at(lang_off + 48, z_block_off_new as u64)?;

                // Tabela de textos: `u32` por entrada, saltando 12 bytes.
                for (index, &offset) in new_offsets[..self.text_count as usize].iter().enumerate() {
                    let at = lang_off + self.text_table_off as u64
                        + (TEXT_TABLE_ENTRY as u64) * index as u64;
                    writer.patch_u32_at(at, offset)?;
                }
                // Tabela de IDs: `u64` em +8, saltando o resto da entrada.
                for (index, &offset) in new_offsets[self.text_count as usize..].iter().enumerate() {
                    let at = lang_off + self.id_table_off as u64
                        + (ID_TABLE_ENTRY as u64) * index as u64
                        + ID_TEXT_OFFSET as u64;
                    writer.patch_u64_at(at, u64::from(offset))?;
                }
            }
            cursor.into_inner()
        };

        Ok(ImportOutcome {
            data,
            stats: ImportStats {
                lines: lines.len(),
                unique,
                extra,
                old_text_block_size: self.text_block_size,
                new_text_block_size,
                new_lang_block_size,
            },
        })
    }

    /// Texto apontado por `offset` (relativo ao bloco de textos), lido até
    /// o `NUL` — ou até o fim do bloco, como no `ReadUTF8String` legado.
    fn text_at(&self, offset: u32) -> &[u8] {
        c_str_at(&self.block, self.text_block_off, offset)
    }
}

/// String terminada em `NUL` a partir de `text_block_off + offset`.
///
/// Offset fora do bloco devolve uma fatia vazia (o `ReadUTF8String` legado
/// também devolveria string vazia ao encontrar o fim do stream).
fn c_str_at(block: &[u8], text_block_off: usize, offset: u32) -> &[u8] {
    let start = match text_block_off.checked_add(offset as usize) {
        Some(start) => start,
        None => return &[],
    };
    if start >= block.len() {
        return &[];
    }
    let rest = &block[start..];
    match rest.iter().position(|&byte| byte == 0) {
        Some(length) => &rest[..length],
        None => rest,
    }
}

fn to_usize(value: u64, context: &'static str) -> Result<usize, DomainError> {
    usize::try_from(value)
        .map_err(|_| DomainError::out_of_range(context, value, usize::MAX as u64))
}

fn count_len(count: usize, entry: usize, context: &'static str) -> Result<usize, DomainError> {
    count
        .checked_mul(entry)
        .ok_or_else(|| DomainError::invalid_data(format!("{context} length overflows")))
}

fn require_range(
    data: &[u8],
    offset: usize,
    len: usize,
    context: &'static str,
) -> Result<(), DomainError> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| DomainError::invalid_data(format!("{context} range overflows")))?;
    if end > data.len() {
        return Err(DomainError::out_of_range(
            context,
            end as u64,
            data.len() as u64,
        ));
    }
    Ok(())
}

fn u32_at(data: &[u8], offset: usize, context: &'static str) -> Result<u32, DomainError> {
    let bytes = bytes_at(data, offset, 4, context)?;
    let mut buffer = [0u8; 4];
    buffer.copy_from_slice(bytes);
    Ok(u32::from_le_bytes(buffer))
}

fn u64_at(data: &[u8], offset: usize, context: &'static str) -> Result<u64, DomainError> {
    let bytes = bytes_at(data, offset, 8, context)?;
    let mut buffer = [0u8; 8];
    buffer.copy_from_slice(bytes);
    Ok(u64::from_le_bytes(buffer))
}

fn bytes_at<'a>(
    data: &'a [u8],
    offset: usize,
    len: usize,
    context: &'static str,
) -> Result<&'a [u8], DomainError> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| DomainError::invalid_data(format!("{context} offset overflows")))?;
    data.get(offset..end)
        .ok_or(DomainError::UnexpectedEof { context })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monta um `.loc` sintético: 2 textos + 2 IDs (um duplicado), bloco Z
    /// e uma cauda arbitrária (para provar que ela é preservada).
    ///
    /// Layout: cabeçalho 16 B | bloco 188 B (header 56 + text table 32 +
    /// ID table 80 + textos 16 + Z 4) | cauda 9 B.
    fn sample_loc() -> Vec<u8> {
        const BLOCK_LEN: usize = 188;
        const TEXT_TABLE_OFF: usize = 56;
        const ID_TABLE_OFF: usize = 88;
        const TEXT_BLOCK_OFF: usize = 168;
        const Z_BLOCK_OFF: usize = 184;

        let mut block = vec![0u8; BLOCK_LEN];
        block[0..8].copy_from_slice(&2u64.to_le_bytes()); // magic
        block[8..12].copy_from_slice(&2u32.to_le_bytes()); // IDCount
        block[12..16].copy_from_slice(&2u32.to_le_bytes()); // TextCount
        block[16..20].copy_from_slice(&16u32.to_le_bytes()); // TextBlockSize
        block[20..24].copy_from_slice(&4u32.to_le_bytes()); // ZBlockSize
        block[24..32].copy_from_slice(&(ID_TABLE_OFF as u64).to_le_bytes());
        block[32..40].copy_from_slice(&(TEXT_TABLE_OFF as u64).to_le_bytes());
        block[40..48].copy_from_slice(&(TEXT_BLOCK_OFF as u64).to_le_bytes());
        block[48..56].copy_from_slice(&(Z_BLOCK_OFF as u64).to_le_bytes());

        // Tabela de textos: "Hello" @0, "World" @6.
        block[56..60].copy_from_slice(&0u32.to_le_bytes());
        block[72..76].copy_from_slice(&6u32.to_le_bytes());

        // Tabela de IDs: "Hello" (duplicado) @0 e "Bye" @12; os demais
        // bytes recebem um padrão para provar que são preservados.
        block[88..128].fill(0x5A);
        block[128..168].fill(0x5A);
        block[96..104].copy_from_slice(&0u64.to_le_bytes());
        block[136..144].copy_from_slice(&12u64.to_le_bytes());

        block[168..184].copy_from_slice(b"Hello\0World\0Bye\0");
        block[184..188].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        let mut file = Vec::new();
        file.extend_from_slice(&LOC_MAGIC.to_le_bytes());
        file.extend_from_slice(&1u32.to_le_bytes()); // LangCount
        file.extend_from_slice(&0u32.to_le_bytes()); // LangBlockIndex
        file.extend_from_slice(&(BLOCK_LEN as u32).to_le_bytes()); // LangBlockSize
        assert_eq!(file.len(), 16);
        file.extend_from_slice(&block);
        file.extend_from_slice(b"TAIL-DATA");
        file
    }

    fn txt_of(lines: &[&[u8]]) -> Vec<u8> {
        let owned: Vec<Vec<u8>> = lines.iter().map(|line| line.to_vec()).collect();
        join_lines(&owned)
    }

    #[test]
    fn parse_reads_the_synthetic_file() -> Result<(), DomainError> {
        let loc = LocFile::parse(Cursor::new(sample_loc()))?;
        assert_eq!(loc.text_count, 2);
        assert_eq!(loc.text_references(), 4);
        assert_eq!(
            loc.texts,
            vec![
                b"Hello".to_vec(),
                b"World".to_vec(),
                b"Hello".to_vec(),
                b"Bye".to_vec()
            ]
        );
        Ok(())
    }

    #[test]
    fn export_deduplicates_in_first_occurrence_order() -> Result<(), DomainError> {
        let loc = LocFile::parse(Cursor::new(sample_loc()))?;
        let outcome = loc.export_txt();
        assert_eq!(outcome.texts, 4);
        assert_eq!(outcome.unique, 3);
        assert_eq!(outcome.txt, txt_of(&[b"Hello", b"World", b"Bye"]));
        Ok(())
    }

    #[test]
    fn import_rewrites_texts_and_preserves_the_rest() -> Result<(), DomainError> {
        let loc = LocFile::parse(Cursor::new(sample_loc()))?;
        let outcome = loc.import_txt(&txt_of(&[b"Ol\xc3\xa1", b"Mundo", b"Tchau"]))?;

        assert_eq!(outcome.stats.lines, 3);
        assert_eq!(outcome.stats.unique, 3);
        assert_eq!(outcome.stats.extra, 0);
        assert_eq!(outcome.stats.old_text_block_size, 16);
        assert_eq!(outcome.stats.new_text_block_size, 17); // 5 + 6 + 6
        assert_eq!(outcome.stats.new_lang_block_size, 189); // 168 + 17 + 4

        // LangBlockSize @12 e TextBlockSize @bloco+16 regravados.
        assert_eq!(u32_at(&outcome.data, 12, "test")?, 189);
        assert_eq!(u32_at(&outcome.data, 16 + 16, "test")?, 17);
        // ZBlockOffset @bloco+48 aponta para o novo bloco Z.
        assert_eq!(u64_at(&outcome.data, 16 + 48, "test")?, 185);
        assert_eq!(
            outcome.data[16 + 185..16 + 189],
            [0xDE, 0xAD, 0xBE, 0xEF]
        );
        // Cauda original preservada byte a byte.
        assert!(outcome.data.ends_with(b"TAIL-DATA"));
        // Padrão 0x5A das entradas de ID não tocado.
        assert_eq!(outcome.data[16 + 88..16 + 96], [0x5A; 8]);

        // O arquivo reimportado reexporta o mesmo TXT.
        let reparsed = LocFile::parse(Cursor::new(outcome.data))?;
        let reexport = reparsed.export_txt();
        assert_eq!(reexport.unique, 3);
        assert_eq!(
            reexport.txt,
            txt_of(&[b"Ol\xc3\xa1", b"Mundo", b"Tchau"])
        );
        Ok(())
    }

    #[test]
    fn import_ignores_extra_lines() -> Result<(), DomainError> {
        let loc = LocFile::parse(Cursor::new(sample_loc()))?;
        let outcome = loc.import_txt(&txt_of(&[
            b"um",
            b"dois",
            b"tres",
            b"quatro",
            b"cinco",
        ]))?;
        assert_eq!(outcome.stats.unique, 3);
        assert_eq!(outcome.stats.extra, 2);
        Ok(())
    }

    #[test]
    fn import_rejects_a_short_txt() -> Result<(), DomainError> {
        let loc = LocFile::parse(Cursor::new(sample_loc()))?;
        let result = loc.import_txt(&txt_of(&[b"apenas", b"duas"]));
        assert!(matches!(result, Err(DomainError::NotFound { .. })));
        Ok(())
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let mut data = sample_loc();
        data[0] = 0x00;
        let result = LocFile::parse(Cursor::new(data));
        assert!(matches!(result, Err(DomainError::InvalidMagic { .. })));
    }

    #[test]
    fn parse_rejects_a_truncated_file() {
        let result = LocFile::parse(Cursor::new(Vec::new()));
        assert!(matches!(result, Err(DomainError::UnexpectedEof { .. })));
    }

    #[test]
    fn parse_rejects_a_broken_language_block() {
        // LangBlockSize aponta além do fim do arquivo.
        let mut data = sample_loc();
        data[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        let result = LocFile::parse(Cursor::new(data));
        assert!(matches!(result, Err(DomainError::OutOfRange { .. })));
    }
}
