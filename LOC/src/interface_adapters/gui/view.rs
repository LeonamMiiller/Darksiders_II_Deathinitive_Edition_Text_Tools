//! View da GUI (Dioxus) — apresentação pura.
//!
//! A view mantém o estado da janela em *signals*, consome o canal de
//! progresso em um `use_future` e traduz cliques em chamadas de ação
//! (diálogos nativos + [`start_export`]/[`start_import`]). Nenhuma regra
//! de negócio mora aqui: só validação da presença dos caminhos e delegação
//! ao [`Controller`].
//!
//! Fluxo dos botões: **Exportar** abre o diálogo de `.loc` e grava o TXT
//! **no mesmo diretório** (`<stem>.txt`, sem diálogo de gravação);
//! **Importar** abre o diálogo de `.txt` e carrega o `.loc` correspondente
//! **da mesma pasta** (`<stem>.loc`) — se não existir, a operação aborta
//! com o caminho completo na mensagem de erro. A saída do import é sempre
//! `<loc>.NEW`.

use std::path::PathBuf;

use dioxus::prelude::*;

use super::controller::Controller;
use crate::infrastructure::gui::GuiMessage;
use crate::interface_adapters::operations;

/// Tamanho máximo do log.
const MAX_LOG_LINES: usize = 4096;

/// Linhas descartadas de uma vez quando o log atinge o limite.
const LOG_TRIM: usize = 1024;

/// Janela principal da ferramenta.
#[component]
pub fn App() -> Element {
    let controller = use_hook(Controller::new);

    // ---- Estado da janela (escrito apenas na thread da UI) ----
    let loc_path = use_signal(|| Option::<PathBuf>::None);
    let txt_path = use_signal(|| Option::<PathBuf>::None);
    let mut logs = use_signal(Vec::<String>::new);
    let mut status = use_signal(|| String::from("Selecione a operação; os diálogos pedem os arquivos."));
    let mut running = use_signal(|| false);

    // ---- Consumo do canal de progresso ----
    //
    // Os casos de uso rodam em threads de trabalho e apenas *enviam*
    // mensagens; aqui elas são aplicadas aos signals na thread da UI.
    {
        let receiver = controller.receiver();
        use_future(move || {
            let receiver = receiver.clone();
            async move {
                while let Some(message) = receiver.recv().await {
                    match message {
                        GuiMessage::Event(line) => {
                            let mut log = logs.write();
                            log.push(line);
                            if log.len() > MAX_LOG_LINES {
                                log.drain(..LOG_TRIM);
                            }
                        }
                        GuiMessage::Finished { summary } => {
                            running.set(false);
                            status.set(summary);
                        }
                        GuiMessage::Failed { error } => {
                            running.set(false);
                            status.set(format!("Falha: {error}"));
                        }
                    }
                }
            }
        });
    }

    // O controlador não é `Copy` (os signals são): uma cópia por ação.
    let export_controller = controller.clone();
    let import_controller = controller.clone();

    // ---- Rótulos derivados do estado ----
    let loc_label = match &*loc_path.read() {
        Some(path) => path.display().to_string(),
        None => String::new(),
    };
    let txt_label = match &*txt_path.read() {
        Some(path) => path.display().to_string(),
        None => String::new(),
    };
    let status_text = (*status.read()).clone();
    let busy_label = if running() {
        "Processando... (o log abaixo acompanha a operação)"
    } else {
        ""
    };

    rsx! {
        div {
            style: "font-family: sans-serif; max-width: 900px; margin: 0 auto; padding: 16px; display: flex; flex-direction: column; gap: 12px;",

            h2 { "Darksiders II Deathinitive Edition — LOC Tool" }
            p {
                style: "color: #555; margin: 0;",
                "Exportação e importação dos textos de arquivos .loc (TXT com escapes <\\n>/<\\r>)"
            }

            div { style: "display: flex; gap: 8px; align-items: center;",
                span { style: "width: 110px;", "Arquivo .loc:" }
                span {
                    style: "flex: 1; font-family: monospace; background: #f4f4f4; border: 1px solid #ccc; padding: 6px 8px; overflow-wrap: anywhere;",
                    "{loc_label}"
                }
            }

            div { style: "display: flex; gap: 8px; align-items: center;",
                span { style: "width: 110px;", "Arquivo .txt:" }
                span {
                    style: "flex: 1; font-family: monospace; background: #f4f4f4; border: 1px solid #ccc; padding: 6px 8px; overflow-wrap: anywhere;",
                    "{txt_label}"
                }
            }

            div { style: "display: flex; gap: 8px; align-items: center;",
                button {
                    disabled: running(),
                    onclick: move |_| start_export(&export_controller, loc_path, txt_path, status, logs, running),
                    "Exportar (.loc -> .txt)"
                }
                button {
                    disabled: running(),
                    onclick: move |_| start_import(&import_controller, loc_path, txt_path, status, logs, running),
                    "Importar (.txt -> .loc.NEW)"
                }
                span { style: "color: #777; font-size: 13px;", "{busy_label}" }
            }

            div { style: "font-weight: 600;", "{status_text}" }

            div {
                style: "height: 340px; overflow-y: auto; background: #1e1e1e; color: #d4d4d4; padding: 8px; font-family: monospace; font-size: 12px;",
                for line in logs.read().iter() {
                    div { style: "white-space: pre-wrap; overflow-wrap: anywhere;", "{line}" }
                }
            }
        }
    }
}

/// Diálogo nativo de abertura de `.loc`.
fn choose_loc_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Arquivo LOC", &["loc"])
        .set_title("Selecionar o arquivo .loc")
        .pick_file()
}

/// Diálogo nativo de abertura de `.txt`.
fn choose_txt_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Arquivo TXT", &["txt"])
        .set_title("Selecionar o arquivo .txt com as traduções")
        .pick_file()
}

/// Fluxo do botão **Exportar**: escolhe o `.loc` e grava o TXT **no mesmo
/// diretório** (`<stem>.txt`), sem diálogo de gravação, disparando o caso
/// de uso em background.
fn start_export(
    controller: &Controller,
    mut loc_path: Signal<Option<PathBuf>>,
    mut txt_path: Signal<Option<PathBuf>>,
    mut status: Signal<String>,
    mut logs: Signal<Vec<String>>,
    mut running: Signal<bool>,
) {
    let loc = match choose_loc_file() {
        Some(path) => path,
        None => {
            status.set(String::from("Exportação cancelada: nenhum arquivo .loc selecionado."));
            return;
        }
    };
    let txt = operations::default_txt_path(&loc);

    loc_path.set(Some(loc.clone()));
    txt_path.set(Some(txt.clone()));
    logs.write().clear();
    status.set(String::from("Exportando..."));
    running.set(true);
    controller.start_export(loc, txt);
}

/// Fluxo do botão **Importar**: escolhe o `.txt` e carrega o `.loc`
/// correspondente **da mesma pasta** (`<stem>.loc`); se não existir, a
/// operação aborta com o caminho completo na mensagem (a checagem acontece
/// no caso de uso, compartilhada com a CLI). A saída será `<loc>.NEW`.
fn start_import(
    controller: &Controller,
    mut loc_path: Signal<Option<PathBuf>>,
    mut txt_path: Signal<Option<PathBuf>>,
    mut status: Signal<String>,
    mut logs: Signal<Vec<String>>,
    mut running: Signal<bool>,
) {
    let txt = match choose_txt_file() {
        Some(path) => path,
        None => {
            status.set(String::from("Importação cancelada: nenhum arquivo .txt selecionado."));
            return;
        }
    };
    let loc = operations::default_loc_path(&txt);

    loc_path.set(Some(loc.clone()));
    txt_path.set(Some(txt.clone()));
    logs.write().clear();
    status.set(String::from("Importando..."));
    running.set(true);
    controller.start_import(loc, txt);
}
