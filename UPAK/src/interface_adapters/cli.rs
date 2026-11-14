//! CLI (clap) — a fachada externa do modo console.
//!
//! A CLI traduz argumentos em chamadas aos casos de uso ([`operations`]) e
//! reporta o progresso no console. A inferência da operação a partir da
//! extensão do arquivo vive em [`operations::infer_operation`].

use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;

use super::operations;
use crate::interface_adapters::format_event;
use crate::use_cases::ports::ProgressEvent;

/// Ferramenta híbrida para extrair e injetar conteúdo em arquivos `.upak`
/// da Darksiders II Deathinitive Edition (formato OBPK + manifest.bin).
#[derive(Parser, Debug)]
#[command(name = "upak", version, about, long_about = None)]
pub struct CliArgs {
    /// Diretório de saída do modo `dump` (também pode ser o positional OUTPUT).
    #[arg(short = 'e', long, value_name = "DIR", conflicts_with = "insert")]
    pub extract: Option<PathBuf>,

    /// Substitui os arquivos da pasta de entrada pelo conteúdo dumpado
    /// (modo `insert`).
    #[arg(short = 'i', long, conflicts_with = "extract")]
    pub insert: bool,

    /// Força a operação em vez de inferir pela extensão do arquivo.
    #[arg(short = 'o', long = "operation", value_enum, value_name = "OP")]
    pub operation: Option<OperationKind>,

    /// Força a abertura da interface gráfica (também é o comportamento
    /// padrão quando nenhum argumento é informado).
    #[arg(long, hide = true)]
    pub gui: bool,

    /// Arquivo de entrada (`.upak` para dump; `.upak` ou `manifest.bin` para insert).
    #[arg(value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Arquivo de saída (opcional; no dump, a pasta de saída acima é usada).
    #[arg(value_name = "OUTPUT")]
    pub output: Option<PathBuf>,
}

impl CliArgs {
    /// A interface gráfica é desejada (`--gui` ou nenhum argumento dado)?
    ///
    /// Mesma semântica do programa Delphi original (0 argumentos -> GUI).
    pub fn wants_gui(&self) -> bool {
        self.gui
            || (self.input.is_none()
                && self.output.is_none()
                && self.extract.is_none()
                && !self.insert
                && self.operation.is_none())
    }
}

/// Operação forçada pela linha de comando.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum OperationKind {
    /// Descompacta o pacote em uma pasta (manifesto + payloads).
    Dump,
    /// Grava uma nova versão do pacote com os payloads de uma pasta.
    Insert,
}

impl From<OperationKind> for operations::Operation {
    fn from(kind: OperationKind) -> Self {
        match kind {
            OperationKind::Dump => operations::Operation::Dump,
            OperationKind::Insert => operations::Operation::Insert,
        }
    }
}

/// Ponto de entrada do modo CLI (`cargo run -- [opções]`).
pub fn run(args: CliArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    let operation = resolve_operation(&args)?;
    let mut progress = ConsoleProgress::new();

    let summary = match operation {
        operations::Operation::Dump => {
            let input = args
                .input
                .as_ref()
                .ok_or("modo `dump`: informe o arquivo `.upak` de entrada")?;
            let output = args
                .extract
                .as_ref()
                .or(args.output.as_ref())
                .ok_or(
                    "modo `dump`: informe a pasta de saída com `-e <DIR>` ou como \
                     OUTPUT (a CLI não infere a saída: o legado geraria `media.TXT`)",
                )?;
            let summary = operations::dump(input, output, &mut progress)?;
            eprintln!("manifesto: {}", operations::manifest_path(input).display());
            summary
        }
        operations::Operation::Insert => {
            let input = args
                .input
                .as_ref()
                .ok_or("modo `insert`: informe o `.upak` ou o `manifest.bin` de entrada")?;
            let output = args
                .output
                .as_ref()
                .ok_or("modo `insert`: informe a pasta de saída")?;
            operations::insert(input, output, &mut progress)?
        }
    };

    eprintln!("concluído: {summary}");
    Ok(())
}

/// Resolve a operação: `--operation` > `-i`/`-e` > inferência pela extensão.
fn resolve_operation(
    args: &CliArgs,
) -> Result<operations::Operation, Box<dyn Error + Send + Sync>> {
    if let Some(kind) = &args.operation {
        return Ok(kind.clone().into());
    }
    // As flags de operação vencem a inferência (mesmo papel do `-e`/`-i`
    // posicionais do legado).
    if args.insert {
        return Ok(operations::Operation::Insert);
    }
    if args.extract.is_some() {
        return Ok(operations::Operation::Dump);
    }
    let input = args
        .input
        .as_ref()
        .ok_or("informe o arquivo de entrada")?;
    operations::infer_operation(input)
        .ok_or_else(|| format!("não foi possível inferir a operação de `{}`", input.display()).into())
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
