//! Darksiders II Deathinitive Edition LOC Tool — ponto de entrada híbrido.
//!
//! O modo de execução é escolhido a partir dos argumentos de linha de comando
//! (mesma semântica do programa Delphi original):
//!
//! * sem argumentos (ou com `--gui`) -> interface gráfica (feature `gui`);
//! * com argumentos                  -> modo CLI.
//!
//! Camadas: `domain` -> `use_cases` -> `interface_adapters` -> `infrastructure`.

mod domain;
mod infrastructure;
mod interface_adapters;
mod use_cases;

use std::error::Error;

use clap::Parser;

use interface_adapters::cli::CliArgs;

fn main() -> std::process::ExitCode {
    let args = CliArgs::parse();
    match bootstrap(args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("ERROR: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Detecta se a aplicação deve subir em modo GUI ou CLI.
///
/// O erro de fronteira é `Send + Sync` para poder atravessar threads de
/// trabalho (a GUI executa os casos de uso fora da thread de UI).
fn bootstrap(args: CliArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    if args.wants_gui() {
        launch_gui()
    } else {
        interface_adapters::cli::run(args)
    }
}

fn launch_gui() -> Result<(), Box<dyn Error + Send + Sync>> {
    #[cfg(feature = "gui")]
    {
        interface_adapters::gui::run()
    }
    #[cfg(not(feature = "gui"))]
    {
        Err(
            "the graphical interface is not part of this build; rebuild with `cargo build --features gui`"
                .into(),
        )
    }
}
