//! Adaptadores de interface: **CLI** (clap) e **GUI** (Dioxus, feature `gui`).
//!
//! Esta camada traduz entradas do usuário (argumentos, clique de botão) em
//! chamadas dos casos de uso e traduz os eventos de progresso de volta em
//! texto/buffers para a tela. Não contém regra de negócio.

pub mod cli;
pub mod operations;

#[cfg(feature = "gui")]
pub mod gui;

use crate::use_cases::ports::ProgressEvent;

/// Converte um evento de progresso em uma linha de texto — usado pela CLI
/// (impressão no console) e pela GUI (linhas do log).
pub fn format_event(event: &ProgressEvent) -> String {
    match event {
        ProgressEvent::Info(message) => message.clone(),
        ProgressEvent::Texts { total, unique } => {
            format!("Texts: {total} reference(s), {unique} unique")
        }
        ProgressEvent::Done { summary } => summary.clone(),
    }
}
