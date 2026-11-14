//! **Portas** (interfaces) dos casos de uso.
//!
//! Contratos abstratos entre a lógica de negócio e os adaptadores: streams
//! binários, arquivos extraídos/ler, códecs de compressão e relatório de
//! progresso. As implementações vivem em `infrastructure` (E/S e zlib) e nos
//! adaptadores de interface (console para a CLI, buffer compartilhado para a
//! GUI). Os casos de uso dependem apenas destes traits — nunca de `std::fs`
//! ou `flate2` diretamente.

use std::error::Error;
use std::io::{Read, Seek, Write};

/// Erro genérico propagado por uma porta (E/S, compressão, arquivo ausente...).
///
/// `Send + Sync` permite que os casos de uso rodem em threads de trabalho
/// (GUI) sem travar a interface.
pub type PortError = Box<dyn Error + Send + Sync>;

/// Fonte binária seekável (manifesto, `.upak`, blocos...).
pub trait BinarySource: Read + Seek + Send {}
impl<T: Read + Seek + Send + ?Sized> BinarySource for T {}

/// Sumidouro binário seekável (arquivos de saída).
pub trait BinarySink: Write + Seek + Send {}
impl<T: Write + Seek + Send + ?Sized> BinarySink for T {}

/// Destino dos arquivos extraídos pelo caso de uso **Dump**.
pub trait DumpSink: Send {
    /// Grava `data` em `relative_path` (caminho de domínio com separador
    /// `\`, ex.: `media\ui\ui_core\ui_core.loc`), criando os diretórios
    /// necessários. O adaptador decide a pasta-raiz e o mapeamento para o
    /// sistema de arquivos.
    fn write_file(&mut self, relative_path: &[u8], data: &[u8]) -> Result<(), PortError>;
}

/// Origem dos arquivos de substituição usados pelo caso de uso **Insert**
/// (`.loc` e `.gfx` previamente exportados pelo Dump).
pub trait PayloadSource: Send {
    /// Lê por inteiro o arquivo `relative_path` (caminho de domínio com `\`).
    fn read_file(&mut self, relative_path: &[u8]) -> Result<Vec<u8>, PortError>;
}

/// Códecs de compressão usados pelos contêineres OBPK.
///
/// A implementação de infraestrutura usa zlib no nível 9 (`zcMax`), o mesmo
/// do `Delutto.Compression.ZLIB` do legado.
pub trait BlobCodec: Send {
    /// Descompacta `compressed` e confere o tamanho declarado pelo formato.
    fn decompress(&self, compressed: &[u8], expected_len: u64) -> Result<Vec<u8>, PortError>;

    /// Compacta `data` em zlib.
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, PortError>;
}

/// Evento de progresso emitido pelos casos de uso.
///
/// A CLI imprime no console; a GUI acumula em um buffer compartilhado que a
/// interface consome periódicamente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// Mensagem informativa (cabeçalho, contagens, avisos).
    Info(String),
    /// Início do processamento de um contêiner OBPK.
    Container {
        /// Posição (1-based) dentro do pacote.
        index: usize,
        /// Total de contêineres do pacote.
        total: usize,
        /// Caminho completo do contêiner (`media\ui\ui_core`).
        path: String,
    },
    /// Contêiner ignorado (chunked, vazio ou sem bloco de dados).
    ContainerSkipped { path: String, reason: &'static str },
    /// Arquivo extraído com sucesso no Dump.
    Extracted { path: String },
    /// Contêiner regravado no `translation.upak` no Insert.
    Imported { path: String, offset: u64 },
    /// Operação concluída, com resumo em texto.
    Done { summary: String },
}

/// Relata o andamento de um caso de uso. Infallível por projeto: uma falha
/// de relatório nunca deve interromper o processamento.
pub trait ProgressReporter: Send {
    fn report(&mut self, event: &ProgressEvent);
}
