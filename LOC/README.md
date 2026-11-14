# RustVersion/LOC — Darksiders II Deathinitive Edition: extrair / inserir arquivos `.loc`

Port em Rust do projeto Delphi `LOC/` (leitor e escritor de arquivos de localização
`.loc` do LE), reescrito com arquitetura limpa: domínio puro, casos de uso
genéricos sobre ports, adaptadores de interface (CLI/GUI) e infraestrutura.

O domínio é independente de I/O e de UI (`src/domain`); a CLI (`src/interface_adapters/cli.rs`)
e a GUI (`src/interface_adapters/gui/*`) são apenas adaptadores sobre os mesmos casos de uso.

## Recursos

- **Extração**: `.loc` → texto (BOM UTF-8 + CRLF), uma linha por texto (CRLF normalizado).
- **Inclusão**: `.loc` + texto → `<input>.NEW` (o bloco de texto é reescrito, o bloco Z e
  o restante do arquivo são preservados).
- **Interface**: CLI com `clap` (feature `cli`, default) e GUI com `dioxus` (feature `gui`).

## Build

```sh
cargo build --release            # CLI (binário `loc`)
cargo build --release --features gui   # CLI + GUI (requer GTK3/WebKit2GTK no Linux)
cargo test
cargo clippy --all-targets
```

## CLI

```
USAGE:
  loc [OPTIONS] <INPUT> [OUTPUT]

OPTIONS:
  -e, --export    Exporta <INPUT>.loc para <OUTPUT>.txt (padrão: <INPUT>.txt,
                  na mesma pasta do .loc)
  -i, --import    Importa o TXT → <INPUT>.NEW
                  INPUT pode ser o .loc (sintaxe legada; OUTPUT = TXT) ou o
                  .txt (o .loc é carregado da mesma pasta; OUTPUT = .loc
                  opcional para sobrescrever a derivação)
      --gui       Abre a interface gráfica
```

Os caminhos são **posicionais** (2º argumento) — os flags são booleanos.
Sem `-e`/`-i`, a operação é inferida pela extensão de `<INPUT>`
(case-insensitive): `.loc` → export, `.txt` → import; outras extensões
produzem erro com dica de uso.

### Exemplos

```sh
# exportar (TXT gravado na mesma pasta do .loc por padrão)
cargo run --release -- -e media/ui_core.loc ui_core.txt
cargo run --release -- -e media/ui_core.loc           # → media/ui_core.txt

# importar passando SÓ o .loc (legado): o TXT é <INPUT>.txt na mesma pasta
cargo run --release -- -i media/ui_core.loc           # → media/ui_core.loc.NEW
cargo run --release -- -i media/ui_core.loc outro.txt # TXT explícito

# importar passando SÓ o .txt: o .loc é carregado da mesma pasta
cargo run --release -- -i media/ui_core.txt           # lê media/ui_core.loc → media/ui_core.loc.NEW

# inferência pela extensão (sem flags)
cargo run --release -- media/ui_core.loc              # exporta
cargo run --release -- media/ui_core.txt              # importa
```

Observações:

- **Export**: o texto usa quebras CRLF entre linhas e **não** tem CRLF final (como o legado);
  o TXT sai no mesmo diretório do `.loc`.
- **Import**: linhas a mais no TXT são ignoradas; TXT com menos textos que o `.loc` é erro
  (`DomainError::NotFound`); falhas removem a saída parcial; a saída é sempre
  `<loc>.NEW` (o legado fazia o mesmo).
- **`.loc` ausente**: quando o `.loc` é derivado do TXT (ou informado) e não existe, a
  operação **aborta** com `arquivo `<caminho completo>` não encontrado, verifique!`
  (exit 1, sem criar arquivos).

## GUI (feature `gui`)

`cargo run --release --features gui` (ou `--gui` / sem argumentos na CLI):

- **Exportar**: diálogo de abertura (`.loc`) → grava `<stem>.txt` **no mesmo diretório**
  do `.loc` (sem diálogo de gravação).
- **Importar**: diálogo de abertura (`.txt`) → carrega `<stem>.loc` **da mesma pasta**;
  se não existir, aborta com `arquivo `<caminho completo>` não encontrado, verifique!`
  na barra de status. A saída é `<loc>.NEW`.
- As linhas do caminho são somente leitura e acompanham a operação; o log de progresso
  e o status ficam visíveis (a janela permanece aberta após o fim).

## Árvore (Clean Architecture + MVC na GUI)

```
RustVersion/LOC/
├── Cargo.toml                  # features: cli (default) / gui
└── src/
    ├── main.rs                 # detecta modo (CLI/GUI) pelos argumentos
    ├── domain/                 # domínio puro, sem I/O
    │   ├── loc.rs              # LocFile: parse, export_txt, import_txt
    │   ├── text.rs             # BreakLine / quebras do Delutto
    │   ├── binary.rs           # Cursor leitor (parsing)
    │   └── error.rs            # DomainError
    ├── use_cases/
    │   ├── export.rs           # ExportUseCase (locais = ports de texto/bytes)
    │   ├── import.rs           # ImportUseCase
    │   └── ports.rs            # TextIo / ByteSink + blanket impls
    ├── interface_adapters/
    │   ├── cli.rs              # adaptador clap + inferência + wants_gui
    │   ├── operations.rs       # composição de casos de uso (compartilhado CLI/GUI)
    │   ├── format_event.rs     # texto do progresso (compartilhado)
    │   └── gui/                # adaptador MVC
    │       ├── controller.rs   # Model (caminhos) + rotinas em std::thread
    │       ├── view.rs         # View (rsx/Dioxus)
    │       └── mod.rs          # raios de progresso (tokio) + launch
    └── infrastructure/
        ├── file_io.rs          # disco: TextIo / ByteSink (std::io)
        └── gui.rs              # runtime dioxus-desktop + diálogo rfd
```

## Formato e validação

- `domain/loc.rs` implementa o parser do cabeçalho (`magic u32 = 0x02434F4C`,
  `lang_count`), do índice de línguas, do bloco da língua (56 bytes: id/text counts,
  tamanhos e offsets das tabelas), da tabela de IDs (40 B) e da tabela de textos
  (16 B), além do export/import.
- `domain/text.rs` porta `BreakLine` (CRLF, CR, LF) e o particionamento de linhas
  (BOM + CRLF, sem linha vazia final) do legado.
- **Validação com arquivos reais** (47 `.loc` extraídos do `media.upak` do jogo):
  - export de `ui_core.loc` → **byte-idêntico** ao TXT esperado (325141 B, 13282 refs);
  - import de `ui_core.loc` + TXT → **byte-idêntico** ao `.NEW` de referência;
  - re-export do `.NEW` → byte-idêntico ao TXT (round-trip);
  - export → import **passando só o `.txt`** → re-export idênticos nos **47/47** arquivos;
  - aborto com mensagem de caminho completo quando o `.loc` derivado não existe (exit 1,
    sem resíduos).

## Desvios do legado (deliberados)

| # | Delphi (`LOC/`) | Rust (`RustVersion/LOC`) | Motivo |
|---|---|---|---|
| 1 | `ZBlockOffset` não era gravado no import | Gravado com o valor novo | Consistência do arquivo reescrito (a tabela de offsets é atualizada) |
| 2 | Extensão `.loc` testada sem `SameText`; qualquer outra extensão era tratada como import (e quebrava) | Inferência case-insensitive: `.loc` → export, `.txt` → import; demais → erro com dica | Prever erros confusos |
| 3 | Saída do import fixa `<INPUT>.NEW` | Idem; o INPUT pode ser o `.loc` ou o `.txt` (o par é derivado da mesma pasta) | Flexibilidade mantendo o nome de saída do legado |
| 4 | Extensão padrão `.TXT` | `.txt` (minúscula) | Padrão de sistemas de arquivos sensíveis a maiúsculas |
| 5 | Arquivos abertos podiam ficar em disco em caso de falha | A saída é removida em falha | Sem resíduos |
| 6 | A janela fechava ao fim da operação | Janela permanece com log/status | Melhor usabilidade |
| 7 | TXT convertido UTF-8 → UTF-16 para `TStrings` | TXT tratado como bytes crus (UTF-8) | Byte-idêntico na re-serialização |
| 8 | Sem validação de faixas | Tabelas/bloco fora da faixa → `DomainError` | Mensagens claras em vez de panic/leitura inválida |
| 9 | GUI: export pedia o destino do TXT (diálogo de gravação); import pedia `.loc` e `.txt` | Export grava `<stem>.txt` ao lado do `.loc`; import pede só o `.txt` e carrega `<stem>.loc` da mesma pasta | Menos cliques; o par de arquivos é sempre o mesmo stem na mesma pasta |
| 10 | `.loc` inexistente → erro cru de E/S (`File not found`) | `arquivo `<caminho completo>` não encontrado, verifique!` | Mensagem clara com o caminho exato procurado |

## Desenvolvimento

- Nenhum `.unwrap()`/`.expect()` no código de produção; todos os caminhos de falha
  retornam `Result` (`DomainError` → `UseCaseError` → `PortError`).
- Testes de domínio (parse/export/import em `.loc` sintético, `BreakLine`, particionamento
  de linhas), de orquestração (inferência, derivação de caminhos, aborto do `.loc`
  ausente) e de caso de uso (bytes em memória via ports) rodam com `cargo test`
  (22 testes) — e com `--features gui` (23, inclui o teste do canal de progresso).
- Para verificar o código da GUI sem instalar GTK/WebKit, use temporariamente
  `dioxus = { version = "0.7", features = ["liveview", "server"] }` e
  `wayland-sys = { version = "0.31", features = ["dlopen"] }` no `Cargo.toml`
  (a compilação real usa `features = ["desktop"]`).
