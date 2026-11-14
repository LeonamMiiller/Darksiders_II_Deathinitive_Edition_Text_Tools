# Darksiders II Deathinitive Edition UPAK Tool — Port Rust

Port Rust da ferramenta Delphi de Delutto ([`UPAK/`](../../UPAK)) para extrair e
injetar conteúdo em pacotes `.upak` (formato OBPK + `manifest.bin`).

Duas interfaces sobre os **mesmos** casos de uso:

| Interface | Stack | Feature |
| --- | --- | --- |
| **CLI** (padrão) | `clap` 4 (derive) | `cli` (default) |
| **GUI** | `dioxus` 0.7 + `rfd` 0.15 | `gui` |

> A GUI é opcional porque depende de bibliotecas de sistema (`webkit2gtk`,
> `gtk3` no Linux). Qualquer build sem a feature `gui` continua compilando e
> rodando apenas o modo console.

---

## 1. Build

```bash
cargo build --release                       # só CLI (padrão)
cargo build --release --features gui        # CLI + GUI
```

## 2. CLI

```
Usage: upak [OPTIONS] [INPUT] [OUTPUT]

  -e, --extract <DIR>     saída do modo dump (também pode ser o OUTPUT)
  -i, --insert            monta translation.upak + manifest.bin.NEW
  -o, --operation <OP>    dump | insert  (força a operação)
      --gui               abre a interface gráfica
  -h, --help
```

### Exemplos

```bash
# Exportar (Dump) — equivalente ao legado "… -e media.upak media"
upak media.upak media                 # operação inferida pela extensão
upak -e media media.upak              # mesma coisa, com a saída explícita

# Importar (Insert) — equivalente ao legado "… -i media.upak media"
upak -i media.upak media              # usa os .loc/.gfx exportados acima
upak manifest.bin media               # inferido: extensão ≠ .upak → insert
```

Saídas geradas pelo **Insert** (sempre ao lado do `.upak` de entrada):

* `translation.upak` — só os contêineres-alvo regravados;
* `manifest.bin.NEW` — manifesto com o pak adicional `translation`.

Regras da CLI:

* `manifest.bin` deve estar na **mesma pasta** do `.upak` (como no legado);
* **inferência da operação**: extensão `.upak` (qualquer caixa) → `dump`;
  qualquer outra extensão → `insert`; sem extensão → erro;
* as flags `-i`/`-e` vencem a inferência (papel do `-e`/`-i` posicionais do
  legado);
* sem argumentos (ou com `--gui`) o programa tenta abrir a GUI.

## 3. GUI

```bash
cargo run --features gui
```

Janela com dois seletores (arquivo de entrada `.upak`/`manifest.bin` e pasta de
trabalho), os botões **Extrair (Dump)** e **Inserir (Insert)**, uma linha de
status e um log rolável com o progresso em tempo real.

Os casos de uso rodam em **thread de trabalho** (E/S + zlib não travam a
janela); o progresso volta por um canal `tokio::sync::mpsc` e é aplicado aos
*signals* da Dioxus por um `use_future` na thread da UI.

## 4. Arquitetura

Clean Architecture + SOLID, com a GUI em MVC adaptado ao ecossistema Rust:

```text
src/
├── main.rs                       # bootstrap: CLI (argumentos) ou GUI (--gui)
├── domain/                       # domínio puro — sem E/S, sem zlib, sem UI
│   ├── binary.rs                 # BinaryReader/BinaryWriter (LE) sobre &[u8]
│   ├── manifest.rs               # parse/escrita do manifest.bin
│   ├── obpk.rs                   # parse do contêiner OBPK + plano de extração
│   └── error.rs                  # DomainError
├── use_cases/                    # aplicação — só conhece as portas (traits)
│   ├── ports.rs                  # BinarySource/BinarySink/DumpSink/PayloadSource/
│   │                             # BlobCodec/ProgressReporter
│   ├── blob.rs                   # leitura/descompactação do blob
│   ├── dump.rs                   # caso de uso Dump   -> DumpReport
│   ├── insert.rs                 # caso de uso Insert -> InsertReport
│   └── error.rs                  # UseCaseError { Domain, Adapter }
├── interface_adapters/           # adaptadores de interface
│   ├── cli.rs                    # clap: argumentos -> casos de uso
│   ├── gui/{controller,view}.rs  # Dioxus: view (rsx) + controlador
│   ├── operations.rs             # composição compartilhada CLI/GUI
│   └── mod.rs                    # format_event() compartilhado
└── infrastructure/               # implementações concretas das portas
    ├── file_io.rs                # fontes/sentinela sobre o sistema de arquivos
    ├── compression.rs            # zlib nível 9 (`zcMax`, como o Delitto)
    └── gui.rs                    # canal de progresso da GUI
```

Fluxo: `interface_adapters` → `use_cases` (portas) → `infrastructure`.
O domínio nunca depende de `std::fs` nem de `flate2`; os casos de uso nunca
dependem de `clap`/`dioxus`.

Regras do projeto respeitadas aqui:

* **nenhum `.unwrap()`/`.expect()` em código de produção** — falhas viram
  `Result`/`Option` propagados com `?`;
* polimorfismo por **`Box<dyn Trait>`** (portas e erros de fronteira
  `Box<dyn Error + Send + Sync>`, para atravessar threads de trabalho);
* E/S indexada com `std::io::{Read, Write, Seek}` e leituras em janela
  limitada (nada de carregar o `media.upak` de 7 GB inteiro em memória).

## 5. Validação (arquivos reais de `GAME_FILES/media`)

| Teste | Resultado |
| --- | --- |
| Dump de `media.upak` (7 GB) | 1138 contêineres, 1129 processados, 9 pulados, **48 arquivos** (47 `.loc` + `font_default.gfx`) em ~40 s |
| `ui_core.loc` extraído | byte-idêntico à referência (8 069 462 bytes) |
| `font_default.gfx` extraído | **114 518 bytes** (completo; o legado truncava em 67 775) |
| Insert a partir do dump | `translation.upak` (8 577 302 B) + `manifest.bin.NEW` (449 236 B) — validador estrutural: **ALL CHECKS PASSED** |
| Round-trip (dump do `translation.upak`) | reproduz `ui_core.loc` e `font_default.gfx` byte-idênticos |
| `cargo check` / `cargo test` | limpos, sem warnings |

O validador usado na conferência do Insert (`/tmp/opencode/validate_insert.py`)
confere: tabela de paks, alvos apontando para o novo pak, blocos
BOD/Null/Langs/Last byte-idênticos e encadeados, `TablesBlockSize =
tables_end - 0x2C`, payloads idênticos aos dumpados e tamanho do `.gfx`.

## 6. Desvios documentados do legado

| # | Legado (Delphi) | Port Rust |
| --- | --- | --- |
| 1 | O `.gfx` era truncado no tamanho declarado na tabela de nomes (67 775 B) | exporta o conteúdo descomprimido **completo** (114 518 B) |
| 2 | Inferência comparava com `.UPAK` literal → `media.upak` virava *import*; arquivo sem extensão também virava *import* | comparação *case-insensitive*; sem extensão → erro |
| 3 | Com 1 argumento, a saída era inferida como `media.TXT` | saída do dump é **obrigatória** (`-e <DIR>` ou `OUTPUT`) |
| 4 | `PakCount` fixado em `3` | `PakCount = original + 1` |
| 5 | Índice do novo pak fixado em `2` (`Paks[2].Name := 'translation'`) | índice = `paks.len()` (nunca `$FF`) |
| 6 | Leitura do blob sem limite (podia varrer o arquivo) | janela limitada a `declarado + declarado/100 + 64 KiB`, com `produzido == declarado` no decompress (teto de 1 GiB) |
| 7 | Manifesto não contíguo era copiado silenciosamente (bytes perdidos) | exige `bod_block_offset == tables_end` |
| 8 | Falha no import deixava saídas parciais para trás | `translation.upak`/`manifest.bin.NEW` são removidos em caso de falha |
| 9 | GUI VCL com diálogo de *salvar arquivo* (`*.txt`) para a saída | GUI Dioxus com seleção de **pastas** (modo `SaveToFolder` do legado) |
| 10 | Nomes em `string` (ANSI/UTF-8 conforme o `ReadUTF8String`) | nomes mantidos como **bytes crus** (`Vec<u8>`) para re-serialização byte-idêntica |

Tudo o que **não** está nesta tabela é fiel ao `Vigil_Havok.Types.UPAK.pas` /
`Vigil_Havok.Types.OBPK.pas`, inclusive a ordem das tabelas, os *seeks*
explícitos da segunda tabela e da tabela de tamanhos, e o blob reescrito como
`u32 tamanho + zlib`.

## 7. Notas de desenvolvimento

* Testes: `cargo test` (CLI) e `cargo test --features gui` (inclui o teste do
  canal de progresso da GUI).
* Esta máquina não tem `webkit2gtk`/`gtk3`, então o código da GUI é validado
  por `cargo check` com uma troca temporária de renderer — para repetir:
  1. em `Cargo.toml`, trocar `features = ["desktop"]` por
     `features = ["liveview", "server"]` em `dioxus`;
  2. adicionar a dependência temporária
     `wayland-sys = { version = "0.31", features = ["dlopen"] }` (dispensa
     `libwayland` do sistema, usada pelo `rfd` via `ashpd`);
  3. `cargo check --features gui` e `cargo test --features gui`;
  4. reverter os dois itens. **(ambos já foram usados: o check saiu limpo e o
     teste do canal passou)**
* `GAME_FILES/` nunca é modificado: os testes usam *symlinks* em `/tmp`.
