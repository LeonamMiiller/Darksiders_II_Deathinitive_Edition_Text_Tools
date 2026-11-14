//! Primitivas de leitura/escrita binária little-endian.
//!
//! Todas as operações usam `std::io::{Read, Write, Seek}` com `read_exact`/
//! `write_all`, garantindo ausência de leituras parciais silenciosas. As
//! falhas (incluindo EOF prematuro) viram [`DomainError`] e são propagadas
//! com `?` — nunca com `unwrap`/`expect`.

use std::io::{Read, Seek, SeekFrom, Write};

use super::error::DomainError;

/// Teto de alocação para leituras de tamanho declarado pelo formato
/// (defesa contra contagens/offsets corrompidos).
const MAX_READ: usize = 1 << 28; // 256 MiB

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

    pub fn seek_to(&mut self, offset: u64) -> Result<(), DomainError> {
        self.inner
            .seek(SeekFrom::Start(offset))
            .map(|_| ())
            .map_err(|source| DomainError::Io {
                context: "seeking the stream",
                source,
            })
    }

    /// Tamanho total do stream (posiciona no fim e devolve o offset).
    pub fn stream_len(&mut self) -> Result<u64, DomainError> {
        self.inner
            .seek(SeekFrom::End(0))
            .map_err(|source| DomainError::Io {
                context: "resolving the stream length",
                source,
            })
    }

    /// Lê exatamente `len` bytes em um `Vec` alocado previamente.
    pub fn read_vec(&mut self, len: usize, context: &'static str) -> Result<Vec<u8>, DomainError> {
        if len > MAX_READ {
            return Err(DomainError::out_of_range(
                context,
                len as u64,
                MAX_READ as u64,
            ));
        }
        let mut buffer = vec![0u8; len];
        self.inner
            .read_exact(&mut buffer)
            .map_err(|source| map_read_error(source, context))?;
        Ok(buffer)
    }

    fn read_array<const N: usize>(&mut self, context: &'static str) -> Result<[u8; N], DomainError> {
        let mut buffer = [0u8; N];
        self.inner
            .read_exact(&mut buffer)
            .map_err(|source| map_read_error(source, context))?;
        Ok(buffer)
    }

    pub fn read_u32(&mut self, context: &'static str) -> Result<u32, DomainError> {
        Ok(u32::from_le_bytes(self.read_array::<4>(context)?))
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
                context: "resolving the output stream position",
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

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), DomainError> {
        self.inner.write_all(bytes).map_err(|source| DomainError::Io {
            context: "writing bytes to the output stream",
            source,
        })
    }

    /// Sobrescreve um `u32` na posição absoluta `offset` e restaura a
    /// posição anterior (equivalente a `WriteUInt32` com `Position :=` do Delphi).
    pub fn patch_u32_at(&mut self, offset: u64, value: u32) -> Result<(), DomainError> {
        let saved = self.position()?;
        self.seek_to(offset)?;
        self.write_bytes(&value.to_le_bytes())?;
        if saved != offset {
            self.seek_to(saved)?;
        }
        Ok(())
    }

    /// Sobrescreve um `u64` na posição absoluta `offset` e restaura a
    /// posição anterior (idem para `WriteUInt64`).
    pub fn patch_u64_at(&mut self, offset: u64, value: u64) -> Result<(), DomainError> {
        let saved = self.position()?;
        self.seek_to(offset)?;
        self.write_bytes(&value.to_le_bytes())?;
        if saved != offset {
            self.seek_to(saved)?;
        }
        Ok(())
    }
}
