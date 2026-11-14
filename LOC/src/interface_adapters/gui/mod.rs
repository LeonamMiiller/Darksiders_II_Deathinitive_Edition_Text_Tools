//! GUI (Dioxus) — adaptador de interface gráfico.
//!
//! A view ([`view`]) só traduz cliques em chamadas do [`controller`]; o
//! controlador dispara os casos de uso em uma thread de trabalho e o
//! progresso volta por um canal ([`crate::infrastructure::gui`]) consumido
//! por um `use_future` na thread da UI. Nenhuma regra de negócio mora aqui.

mod controller;

#[cfg(feature = "gui")]
mod view;

/// Sobe a interface gráfica e bloqueia até a janela ser fechada.
#[cfg(feature = "gui")]
pub fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dioxus::launch(view::App);
    Ok(())
}
