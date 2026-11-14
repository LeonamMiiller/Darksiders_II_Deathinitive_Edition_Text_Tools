//! View da GUI (Dioxus) — apresentação pura.
//!
//! A view mantém o estado da janela em *signals*, consome o canal de
//! progresso em um `use_future` e traduz cliques em chamadas de ação
//! ([`start_dump`], [`start_insert`] e os diálogos de seleção). Nenhuma
//! regra de negócio mora aqui: só validação da presença dos caminhos e
//! delegação ao [`Controller`].

use std::path::PathBuf;

use dioxus::prelude::*;

use super::controller::Controller;
use crate::infrastructure::gui::GuiMessage;

/// Tamanho máximo do log (uma extração longa emite ~1,2 mil linhas).
const MAX_LOG_LINES: usize = 4096;

/// Linhas descartadas de uma vez quando o log atinge o limite.
const LOG_TRIM: usize = 1024;

/// Janela principal da ferramenta.
#[component]
pub fn App() -> Element {
    let controller = use_hook(Controller::new);

    // ---- Estado da janela (escrito apenas na thread da UI) ----
    let mut input = use_signal(|| Option::<PathBuf>::None);
    let mut output = use_signal(|| Option::<PathBuf>::None);
    let mut logs = use_signal(Vec::<String>::new);
    let mut status = use_signal(|| String::from("Selecione os arquivos e a operação."));
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
    let dump_controller = controller.clone();
    let insert_controller = controller.clone();

    // ---- Rótulos derivados do estado ----
    let input_label = match &*input.read() {
        Some(path) => path.display().to_string(),
        None => String::new(),
    };
    let output_label = match &*output.read() {
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

            h2 { "Darksiders II Deathinitive Edition — UPAK Tool" }
            p {
                style: "color: #555; margin: 0;",
                "Extração (Dump) e injeção (Insert) dos arquivos .loc/.gfx de pacotes .upak"
            }

            div { style: "display: flex; gap: 8px; align-items: center;",
                span { style: "width: 110px;", "Arquivo:" }
                span {
                    style: "flex: 1; font-family: monospace; background: #f4f4f4; border: 1px solid #ccc; padding: 6px 8px; overflow-wrap: anywhere;",
                    "{input_label}"
                }
                button {
                    disabled: running(),
                    onclick: move |_| {
                        if let Some(path) = choose_input_file() {
                            input.set(Some(path));
                        }
                    },
                    "Escolher..."
                }
            }

            div { style: "display: flex; gap: 8px; align-items: center;",
                span { style: "width: 110px;", "Pasta:" }
                span {
                    style: "flex: 1; font-family: monospace; background: #f4f4f4; border: 1px solid #ccc; padding: 6px 8px; overflow-wrap: anywhere;",
                    "{output_label}"
                }
                button {
                    disabled: running(),
                    onclick: move |_| {
                        if let Some(path) = choose_folder("Selecionar a pasta") {
                            output.set(Some(path));
                        }
                    },
                    "Escolher..."
                }
            }

            div { style: "display: flex; gap: 8px; align-items: center;",
                button {
                    disabled: running(),
                    onclick: move |_| start_dump(&dump_controller, input, output, status, logs, running),
                    "Extrair (Dump)"
                }
                button {
                    disabled: running(),
                    onclick: move |_| start_insert(&insert_controller, input, output, status, logs, running),
                    "Inserir (Insert)"
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

/// Diálogo nativo de escolha do arquivo de entrada (`.upak`/`manifest.bin`).
fn choose_input_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Pacote UPAK / manifesto", &["upak", "bin"])
        .set_title("Selecionar o arquivo de entrada")
        .pick_file()
}

/// Diálogo nativo de escolha de pasta (destino do Dump / payloads do Insert).
fn choose_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

/// Valida os caminhos escolhidos e dispara o **Dump** em background.
fn start_dump(
    controller: &Controller,
    input: Signal<Option<PathBuf>>,
    output: Signal<Option<PathBuf>>,
    mut status: Signal<String>,
    mut logs: Signal<Vec<String>>,
    mut running: Signal<bool>,
) {
    let input_path = match &*input.read() {
        Some(path) => path.clone(),
        None => {
            status.set(String::from("Escolha o arquivo .upak de entrada."));
            return;
        }
    };
    let output_dir = match &*output.read() {
        Some(path) => path.clone(),
        None => {
            status.set(String::from("Escolha a pasta de destino da extração."));
            return;
        }
    };

    logs.write().clear();
    status.set(String::from("Extraindo..."));
    running.set(true);
    controller.start_dump(input_path, output_dir);
}

/// Valida os caminhos escolhidos e dispara o **Insert** em background.
fn start_insert(
    controller: &Controller,
    input: Signal<Option<PathBuf>>,
    output: Signal<Option<PathBuf>>,
    mut status: Signal<String>,
    mut logs: Signal<Vec<String>>,
    mut running: Signal<bool>,
) {
    let input_path = match &*input.read() {
        Some(path) => path.clone(),
        None => {
            status.set(String::from("Escolha o .upak (ou o manifest.bin) de entrada."));
            return;
        }
    };
    let payload_dir = match &*output.read() {
        Some(path) => path.clone(),
        None => {
            status.set(String::from(
                "Escolha a pasta com os arquivos exportados pelo Dump.",
            ));
            return;
        }
    };

    logs.write().clear();
    status.set(String::from("Inserindo..."));
    running.set(true);
    controller.start_insert(input_path, payload_dir);
}
