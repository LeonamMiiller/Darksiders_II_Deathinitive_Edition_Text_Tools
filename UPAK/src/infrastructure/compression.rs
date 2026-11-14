//! Implementação da porta [`BlobCodec`] com zlib (nível 9 — o `zcMax` do
//! `Delutto.Compression.ZLIB` do Delphi).
//!
//! A descompressão valida o tamanho declarado pelo próprio formato contra o
//! produzido pelo zlib e impõe um teto de alocação, de modo que um cabeçalho
//! corrompido não provoca alocação descontrolada.

use std::io::Read;

use flate2::read::{ZlibDecoder, ZlibEncoder};
use flate2::Compression;

use crate::use_cases::ports::{BlobCodec, PortError};

/// Nível de compressão equivalente ao `zcMax` legado.
const LEVEL: u32 = 9;

/// Teto de bytes descompactados aceito por uma leitura (defesa contra
/// cabeçalhos corrompidos).
const MAX_OUTPUT: u64 = 1 << 30; // 1 GiB

/// Pré-alocação máxima do buffer de saída (o restante cresce sob demanda).
const MAX_PREALLOC: usize = 16 << 20; // 16 MiB

/// Códec zlib dos contêineres OBPK.
#[derive(Debug, Clone, Copy)]
pub struct ZlibCodec {
    level: u32,
}

impl Default for ZlibCodec {
    fn default() -> Self {
        Self { level: LEVEL }
    }
}

impl BlobCodec for ZlibCodec {
    fn decompress(&self, compressed: &[u8], expected_len: u64) -> Result<Vec<u8>, PortError> {
        if expected_len > MAX_OUTPUT {
            return Err(format!(
                "declared payload of {expected_len} bytes exceeds the {MAX_OUTPUT} byte limit"
            )
            .into());
        }

        let mut output = Vec::with_capacity((expected_len as usize).min(MAX_PREALLOC));
        ZlibDecoder::new(compressed).read_to_end(&mut output)?;

        if output.len() as u64 != expected_len {
            return Err(format!(
                "zlib payload mismatch: the format declares {expected_len} bytes, {} were decompressed",
                output.len()
            )
            .into());
        }
        Ok(output)
    }

    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, PortError> {
        let mut output = Vec::new();
        ZlibEncoder::new(data, Compression::new(self.level)).read_to_end(&mut output)?;
        Ok(output)
    }
}
