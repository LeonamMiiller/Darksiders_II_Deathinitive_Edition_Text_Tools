//! Controlador da GUI: conecta os controles da [`super::view`] aos casos
//! de uso.
//!
//! Os casos de uso são bloqueantes (leitura/gravação de arquivos), então
//! cada operação roda em uma **thread de trabalho**; o progresso e o
//! resultado voltam pelo canal de [`GuiMessage`] e são consumidos pela UI
//! na thread principal. O controlador não toca em *signals* — quem escreve
//! na tela é sempre a view.

use std::path::PathBuf;

use crate::infrastructure::gui::{channel, GuiMessage, GuiProgress, GuiReceiver, GuiSender};
use crate::interface_adapters::operations;

/// Controlador compartilhado entre a view e as threads de trabalho.
///
/// Precisa ser [`Clone`] porque o hook `use_hook` da Dioxus devolve um
/// clone a cada renderização.
#[derive(Clone)]
pub struct Controller {
    tx: GuiSender,
    receiver: GuiReceiver,
}

impl Controller {
    /// Cria o canal de progresso usado por esta janela.
    pub fn new() -> Self {
        let (tx, receiver) = channel();
        Self { tx, receiver }
    }

    /// Receptor do canal — a view o consome dentro de um `use_future`.
    pub fn receiver(&self) -> GuiReceiver {
        self.receiver.clone()
    }

    /// Dispara o **Export** em uma thread de trabalho.
    ///
    /// `loc` é o arquivo `.loc` de entrada e `txt` o destino do TXT.
    pub fn start_export(&self, loc: PathBuf, txt: PathBuf) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut progress = GuiProgress::new(tx.clone());
            let message = match operations::export(&loc, &txt, &mut progress) {
                Ok(summary) => GuiMessage::Finished { summary },
                Err(error) => GuiMessage::Failed {
                    error: error.to_string(),
                },
            };
            let _ = tx.send(message);
        });
    }

    /// Dispara o **Import** em uma thread de trabalho.
    ///
    /// `loc` é o `.loc` a regravar e `txt` o arquivo de tradução; a saída
    /// é `<loc>.NEW` (regra do legado).
    pub fn start_import(&self, loc: PathBuf, txt: PathBuf) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut progress = GuiProgress::new(tx.clone());
            let message = match operations::import(&loc, &txt, &mut progress) {
                Ok(summary) => GuiMessage::Finished { summary },
                Err(error) => GuiMessage::Failed {
                    error: error.to_string(),
                },
            };
            let _ = tx.send(message);
        });
    }
}
