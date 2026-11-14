//! **Portas** (interfaces) dos casos de uso.
//!
//! Contratos abstratos entre a lógica de negócio e os adaptadores: streams
//! binários e relatório de progresso. As implementações vivem em
//! `infrastructure` (E/S sobre o sistema de arquivos) e nos adaptadores de
//! interface (console para a CLI, canal para a GUI). Os casos de uso
//! dependem apenas destes traits — nunca de `std::fs` diretamente.

use std::error::Error;
use std::io::{Read, Seek, Write};

/// Erro genérico propagado por uma porta (E/S, arquivo ausente...).
///
/// `Send + Sync` permite que os casos de uso rodem em threads de trabalho
/// (GUI) sem travar a interface.
pub type PortError = Box<dyn Error + Send + Sync>;

/// Fonte binária seekável (`.loc` e `.txt` de entrada).
pub trait BinarySource: Read + Seek + Send {}
impl<T: Read + Seek + Send + ?Sized> BinarySource for T {}

/// Sumidouro binário seekável (`.txt` exportado e `.loc.NEW`).
pub trait BinarySink: Write + Seek + Send {}
impl<T: Write + Seek + Send + ?Sized> BinarySink for T {}

/// Evento de progresso emitido pelos casos de uso.
///
/// A CLI imprime no console; a GUI acumula em um canal consumido pela
/// interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// Mensagem informativa (cabeçalho da operação, caminhos...).
    Info(String),
    /// Textos processados: total de referências e quantos são únicos.
    Texts { total: usize, unique: usize },
    /// Operação concluída, com resumo em texto.
    Done { summary: String },
}

/// Relata o andamento de um caso de uso. Infallível por projeto: uma falha
/// de relatório nunca deve interromper o processamento.
pub trait ProgressReporter: Send {
    fn report(&mut self, event: &ProgressEvent);
}
