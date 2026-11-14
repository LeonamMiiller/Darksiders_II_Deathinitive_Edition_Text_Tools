//! Primitivas de leitura/escrita binária little-endian.
//!
//! Todas as operações usam `std::io::{Read, Write, Seek}` com `read_exact`/
//! `write_all`, garantindo ausência de leituras parciais silenciosas. As
//! falhas (incluindo EOF prematuro) viram [`DomainError`] e são propagadas
//! com `?` — nunca com `unwrap`/`expect`.

use std::io::{Read, Seek, SeekFrom, Write};

use super::error::DomainError;

/// Teto de alocação para leituras de tamanho prefixado (defesa contra
/// contagens corrompidas).
const MAX_PREFIXED_READ: usize = 1 << 28; // 256 MiB

fn map_read_error(source: std::io::Error, context: &'static str) -> DomainError {
    if source.kind() == std::io::ErrorKind::UnexpectedEof {
        DomainError::UnexpectedEof { context }
    } else {
        DomainError::Io { context, source }
    }
}

/// Leitor sequencial/aleatório sobre qualquer `Read + Seek`.
pub struct BinaryReader<R> {
    inner: R,
}

impl<R: Read + Seek> BinaryReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    pub fn position(&mut self) -> Result<u64, DomainError> {
        self.inner
            .stream_position()
            .map_err(|source| DomainError::Io {
                context: "resolving the stream position",
                source,
            })
    }

    pub fn seek_to(&mut self, offset: u64) -> Result<(), DomainError> {
        self.inner
            .seek(SeekFrom::Start(offset))
            .map(|_| ())
            .map_err(|source| DomainError::Io {
                context: "seeking the stream",
                source,
            })
    }

    pub fn seek_by(&mut self, delta: i64) -> Result<(), DomainError> {
        self.inner
            .seek(SeekFrom::Current(delta))
            .map(|_| ())
            .map_err(|source| DomainError::Io {
                context: "seeking the stream",
                source,
            })
    }

    /// Tamanho total do stream (posiciona no fim e devolve o offset).
    ///
    /// Usado para limitar a janela de leitura dos blobs comprimidos.
    pub fn stream_len(&mut self) -> Result<u64, DomainError> {
        self.inner
            .seek(SeekFrom::End(0))
            .map_err(|source| DomainError::Io {
                context: "resolving the stream length",
                source,
            })
    }

    /// Avança `len` bytes a partir da posição atual.
    pub fn skip(&mut self, len: u64) -> Result<(), DomainError> {
        let delta = i64::try_from(len).map_err(|_| {
            DomainError::out_of_range("skip length", len, i64::MAX as u64)
        })?;
        self.seek_by(delta)
    }

    pub fn read_array<const N: usize>(&mut self, context: &'static str) -> Result<[u8; N], DomainError> {
        let mut buffer = [0u8; N];
        self.inner
            .read_exact(&mut buffer)
            .map_err(|source| map_read_error(source, context))?;
        Ok(buffer)
    }

    /// Lê exatamente `len` bytes em um `Vec` alocado previamente.
    pub fn read_vec(&mut self, len: usize, context: &'static str) -> Result<Vec<u8>, DomainError> {
        if len > MAX_PREFIXED_READ {
            return Err(DomainError::out_of_range(
                context,
                len as u64,
                MAX_PREFIXED_READ as u64,
            ));
        }
        let mut buffer = vec![0u8; len];
        self.inner
            .read_exact(&mut buffer)
            .map_err(|source| map_read_error(source, context))?;
        Ok(buffer)
    }

    pub fn read_u8(&mut self, context: &'static str) -> Result<u8, DomainError> {
        Ok(self.read_array::<1>(context)?[0])
    }

    pub fn read_u16(&mut self, context: &'static str) -> Result<u16, DomainError> {
        Ok(u16::from_le_bytes(self.read_array::<2>(context)?))
    }

    pub fn read_u32(&mut self, context: &'static str) -> Result<u32, DomainError> {
        Ok(u32::from_le_bytes(self.read_array::<4>(context)?))
    }

    pub fn read_u64(&mut self, context: &'static str) -> Result<u64, DomainError> {
        Ok(u64::from_le_bytes(self.read_array::<8>(context)?))
    }
}

/// Escritor sequencial/aleatório sobre qualquer `Write + Seek`.
pub struct BinaryWriter<W> {
    inner: W,
}

impl<W: Write + Seek> BinaryWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    pub fn position(&mut self) -> Result<u64, DomainError> {
        self.inner
            .stream_position()
            .map_err(|source| DomainError::Io {
                context: "resolving the stream position",
                source,
            })
    }

    pub fn seek_to(&mut self, offset: u64) -> Result<(), DomainError> {
        self.inner
            .seek(SeekFrom::Start(offset))
            .map(|_| ())
            .map_err(|source| DomainError::Io {
                context: "seeking the output stream",
                source,
            })
    }

    pub fn seek_to_end(&mut self) -> Result<(), DomainError> {
        self.inner
            .seek(SeekFrom::End(0))
            .map(|_| ())
            .map_err(|source| DomainError::Io {
                context: "seeking the output stream",
                source,
            })
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), DomainError> {
        self.inner.write_all(bytes).map_err(|source| DomainError::Io {
            context: "writing bytes to the output stream",
            source,
        })
    }

    pub fn write_u8(&mut self, value: u8) -> Result<(), DomainError> {
        self.write_bytes(&[value])
    }

    pub fn write_u16(&mut self, value: u16) -> Result<(), DomainError> {
        self.write_bytes(&value.to_le_bytes())
    }

    pub fn write_u32(&mut self, value: u32) -> Result<(), DomainError> {
        self.write_bytes(&value.to_le_bytes())
    }

    pub fn write_u64(&mut self, value: u64) -> Result<(), DomainError> {
        self.write_bytes(&value.to_le_bytes())
    }

    /// Sobrescreve um `u32` na posição absoluta `offset` e restaura a
    /// posição anterior (equivalente a `SetUInt32` do Delphi).
    pub fn patch_u32_at(&mut self, offset: u64, value: u32) -> Result<(), DomainError> {
        let saved = self.position()?;
        self.seek_to(offset)?;
        self.write_u32(value)?;
        if saved != offset {
            self.seek_to(saved)?;
        }
        Ok(())
    }
}
