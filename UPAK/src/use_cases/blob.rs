//! Helpers compartilhados pelos casos de uso **Dump** e **Insert**:
//! leitura do blob comprimido de um contêiner OBPK e decodificação do
//! payload `.gfx`.
//!
//! A leitura é limitada por uma *janela* calculada a partir do tamanho
//! declarado pelo próprio formato: o dado comprimido nunca pode exceder o
//! tamanho descomprimido declarado por mais do que a margem de sobressinal
//! do zlib, nem o fim do arquivo — o que torna a operação resistente a
//! arquivos corrompidos (o legado lia até o fim do stream sem limite).

use std::io::{Read, Seek};

use super::error::UseCaseError;
use super::ports::BlobCodec;
use crate::domain::binary::BinaryReader;
use crate::domain::error::DomainError;
use crate::domain::obpk::Obpk;

/// Folga adicionada à janela de leitura (cabeçalho zlib + sobressinal).
const READ_MARGIN: u64 = 64 * 1024;

/// Proporção máxima de expansão do zlib sobre o tamanho declarado.
const ZLIB_EXPANSION_PERCENT: u64 = 1;

/// Lê e descompacta o blob de dados de um contêiner OBPK.
///
/// * `reader` — stream do `.upak`, posicionável;
/// * `base_offset` — offset do contêiner dentro do stream;
/// * `obpk` — cabeçalho já analisado (fornece `data_offset`);
/// * `stream_len` — tamanho total do stream (para limitar a janela);
/// * `codec` — códec zlib (porta).
pub fn read_blob<R: Read + Seek>(
    reader: &mut BinaryReader<R>,
    base_offset: u64,
    obpk: &Obpk,
    stream_len: u64,
    codec: &dyn BlobCodec,
) -> Result<Vec<u8>, UseCaseError> {
    let data_start = base_offset
        .checked_add(u64::from(obpk.header.data_offset))
        .ok_or_else(|| DomainError::invalid_data("OBPK data offset overflows the stream"))?;
    if data_start.saturating_add(4) > stream_len {
        return Err(DomainError::invalid_data(format!(
            "OBPK data block at {data_start} is outside the stream ({stream_len} bytes)"
        ))
        .into());
    }

    reader.seek_to(data_start)?;
    let declared = u64::from(reader.read_u32("OBPK uncompressed size")?);

    let remaining = stream_len.saturating_sub(data_start + 4);
    let budget = declared
        .saturating_add(declared / 100 * ZLIB_EXPANSION_PERCENT)
        .saturating_add(READ_MARGIN);
    let window = remaining.min(budget);

    let compressed = reader.read_vec(window as usize, "OBPK compressed data")?;
    Ok(codec.decompress(&compressed, declared)?)
}

/// Decodifica o payload de um arquivo `.gfx`
/// (`u32 tamanho_descomprimido + zlib`).
///
/// **Desvio documentado do legado:** o Delphi exportava apenas
/// `NameTable[I].Size` bytes do conteúdo descomprimido (truncando, por
/// exemplo, 114518 bytes para 67775 em `ui_font_default`); aqui o conteúdo
/// completo é preservado.
pub fn decode_gfx_payload(payload: &[u8], codec: &dyn BlobCodec) -> Result<Vec<u8>, UseCaseError> {
    let header = payload
        .get(0..4)
        .ok_or_else(|| DomainError::invalid_data("gfx payload is smaller than its size header"))?;
    let declared = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    Ok(codec.decompress(&payload[4..], u64::from(declared))?)
}
