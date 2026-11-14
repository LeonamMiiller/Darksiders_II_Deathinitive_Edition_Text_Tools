//! Canal de mensagens entre os casos de uso (threads de trabalho) e a
//! interface Dioxus.
//!
//! Os casos de uso são bloqueantes (E/S + zlib), então a GUI os executa em
//! uma thread de trabalho; o progresso volta por um canal `mpsc` e é
//! consumido por um `use_future` na thread da UI. Nada é escrito em
//! *signals* de fora da thread da UI.

use std::sync::Arc;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

use crate::interface_adapters::format_event;
use crate::use_cases::ports::{ProgressEvent, ProgressReporter};

/// Mensagem enviada de uma operação para a interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiMessage {
    /// Linha de log (evento de progresso já formatado por
    /// [`crate::interface_adapters::format_event`]).
    Event(String),
    /// Operação concluída com sucesso (resumo vindo do caso de uso).
    Finished { summary: String },
    /// Operação falhou (mensagem pronta para exibir).
    Failed { error: String },
}

/// Lado emissor do canal — clonável e seguro para enviar de qualquer thread.
pub type GuiSender = UnboundedSender<GuiMessage>;

/// Cria o par emissor/receptor do canal da GUI.
pub fn channel() -> (GuiSender, GuiReceiver) {
    let (tx, rx) = unbounded_channel();
    (
        tx,
        GuiReceiver {
            rx: Arc::new(Mutex::new(rx)),
        },
    )
}

/// Lado receptor do canal.
///
/// O receptor fica atrás de `Arc<Mutex<_>>` porque o hook `use_hook` da
/// Dioxus exige `Clone` (devolve um clone) e a UI precisa de uma cópia
/// compartilhada para consumir as mensagens dentro de um future.
#[derive(Debug, Clone)]
pub struct GuiReceiver {
    rx: Arc<Mutex<UnboundedReceiver<GuiMessage>>>,
}

impl GuiReceiver {
    /// Espera a próxima mensagem (`None` quando todos os emitters foram
    /// descartados e o canal fechou).
    pub async fn recv(&self) -> Option<GuiMessage> {
        let mut receiver = self.rx.lock().await;
        receiver.recv().await
    }
}

/// Reporter de progresso que encaminha os eventos para a UI.
///
/// Implementa [`ProgressReporter`] — o mesmo contrato usado pela CLI —
/// apenas traduzindo o evento em [`GuiMessage::Event`].
pub struct GuiProgress {
    tx: GuiSender,
}

impl GuiProgress {
    pub fn new(tx: GuiSender) -> Self {
        Self { tx }
    }
}

impl ProgressReporter for GuiProgress {
    fn report(&mut self, event: &ProgressEvent) {
        // O canal só falha se a interface já foi fechada; nesse caso não há
        // ninguém para quem reportar e o processamento pode continuar.
        let _ = self.tx.send(GuiMessage::Event(format_event(event)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::ports::ProgressEvent;

    /// O reporter empurra eventos formatados e a UI os recebe na ordem.
    #[tokio::test(flavor = "current_thread")]
    async fn progress_reaches_the_receiver_in_order() {
        let (tx, receiver) = channel();
        let mut progress = GuiProgress::new(tx.clone());

        progress.report(&ProgressEvent::Info(String::from("File count: 2")));
        progress.report(&ProgressEvent::Extracted {
            path: String::from("media\\ui.loc"),
        });
        let _ = tx.send(GuiMessage::Finished {
            summary: String::from("done"),
        });

        assert_eq!(
            receiver.recv().await,
            Some(GuiMessage::Event(String::from("File count: 2")))
        );
        assert_eq!(
            receiver.recv().await,
            Some(GuiMessage::Event(String::from(
                "Extracted media\\ui.loc"
            )))
        );
        assert_eq!(
            receiver.recv().await,
            Some(GuiMessage::Finished {
                summary: String::from("done")
            })
        );
        // Todos os emitters descartados -> canal fechado.
        drop(progress);
        drop(tx);
        assert_eq!(receiver.recv().await, None);
    }
}
