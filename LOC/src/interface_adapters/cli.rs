//! CLI (clap) — a fachada externa do modo console.
//!
//! A CLI traduz argumentos em chamadas aos casos de uso ([`operations`]) e
//! reporta o progresso no console. A inferência da operação a partir da
//! extensão do arquivo vive em [`operations::infer_operation`].

use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;

use super::operations::{self, Operation};
use crate::interface_adapters::format_event;
use crate::use_cases::ports::ProgressEvent;

/// Ferramenta híbrida para exportar e importar os textos de arquivos `.loc`
/// da Darksiders II Deathinitive Edition.
#[derive(Parser, Debug)]
#[command(name = "loc", version, about, long_about = None)]
pub struct CliArgs {
    /// Exporta os textos do `.loc` para um TXT (equivale ao `-e` legado).
    #[arg(short = 'e', long, conflicts_with = "import")]
    pub export: bool,

    /// Importa as traduções de um TXT para `<loc>.NEW`
    /// (equivale ao `-i` legado).
    #[arg(short = 'i', long, conflicts_with = "export")]
    pub import: bool,

    /// Força a abertura da interface gráfica (também é o comportamento
    /// padrão quando nenhum argumento é informado).
    #[arg(long, hide = true)]
    pub gui: bool,

    /// Arquivo de entrada: `.loc` (export/import) ou `.txt` (import — o
    /// `.loc` correspondente é procurado na mesma pasta).
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Export: destino do TXT (padrão `<INPUT>.txt`, mesma pasta).
    /// Import: o `.loc` a regravar quando INPUT é um `.txt`
    /// (padrão: `<INPUT>.loc` na mesma pasta).
    #[arg(value_name = "OUTPUT")]
    pub output: Option<PathBuf>,
}

impl CliArgs {
    /// A interface gráfica é desejada (`--gui` ou nenhum argumento dado)?
    ///
    /// Mesma semântica do programa Delphi original (0 argumentos -> GUI).
    pub fn wants_gui(&self) -> bool {
        self.gui
            || (self.input.is_none() && self.output.is_none() && !self.export && !self.import)
    }
}

/// Ponto de entrada do modo CLI (`cargo run -- [opções]`).
pub fn run(args: CliArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    let operation = resolve_operation(&args)?;
    let mut progress = ConsoleProgress::new();

    let input = args
        .input
        .as_ref()
        .ok_or("informe o arquivo de entrada (`.loc` ou `.txt`) ou use `-e`/`-i`")?;

    let summary = match operation {
        Operation::Export => {
            let output = match &args.output {
                Some(path) => path.clone(),
                None => operations::default_txt_path(input),
            };
            operations::export(input, &output, &mut progress)?
        }
        Operation::Import => {
            let (loc, txt) = operations::resolve_import_paths(input, args.output.as_deref());
            operations::import(&loc, &txt, &mut progress)?
        }
    };

    eprintln!("concluído: {summary}");
    Ok(())
}

/// Resolve a operação: `-i`/`-e` > inferência pela extensão.
///
/// As flags de operação vencem a inferência (mesmo papel do `-e`/`-i`
/// posicionais do legado, que exigiam três argumentos para funcionar).
fn resolve_operation(
    args: &CliArgs,
) -> Result<Operation, Box<dyn Error + Send + Sync>> {
    if args.export {
        return Ok(Operation::Export);
    }
    if args.import {
        return Ok(Operation::Import);
    }
    let input = args
        .input
        .as_ref()
        .ok_or("informe o arquivo de entrada (`.loc` ou `.txt`) ou use `-e`/`-i`")?;
    operations::infer_operation(input).ok_or_else(|| {
        format!(
            "não foi possível inferir a operação de `{}` (esperado um arquivo .loc ou .txt)",
            input.display()
        )
        .into()
    })
}

/// Reporter de progresso para o console (uma linha por evento).
struct ConsoleProgress {
    started: Instant,
}

impl ConsoleProgress {
    fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl crate::use_cases::ports::ProgressReporter for ConsoleProgress {
    fn report(&mut self, event: &ProgressEvent) {
        eprintln!("{}s {}", self.started.elapsed().as_secs(), format_event(event));
    }
}
