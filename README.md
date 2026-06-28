# rMail

Um cliente de e-mail de desktop **rápido**, **simples** e **elegante**, escrito em
Rust sobre o **GPUI** (o framework de UI acelerado por GPU do
[Zed](https://github.com/zed-industries/zed)).

> **Status:** protótipo visual (mock). A interface — inspirada no cliente de
> e-mail do macOS — já está montada com dados de exemplo, para validar o layout e
> a velocidade de inicialização. A lógica de e-mail (IMAP/POP3/Gmail) será
> implementada em etapas. Veja [`docs/PLANEJAMENTO.md`](docs/PLANEJAMENTO.md).

## Destaques

- **Inicialização rápida**: binário nativo, sem Electron/web runtime.
- **Layout estilo Mail do macOS**: barra lateral de contas, lista de mensagens e
  painel de leitura, com toolbar unificada e barra de status.
- **Dois temas**: claro (VSCode *Light Modern*) e escuro (VSCode *Dark Modern*),
  alternáveis em tempo real.
- **Multiplataforma**: Windows, Linux e macOS.

## Como rodar

Requer a toolchain Rust definida em `rust-toolchain.toml` (instalada
automaticamente pelo `rustup`).

```bash
cargo run -p rmail        # abre o protótipo visual
cargo test --workspace    # roda os testes
cargo clippy --workspace  # lint
```

## Estrutura

```
crates/
├── theme/   # temas e cores (claro/escuro)
├── ui/      # componentes visuais reutilizáveis (Label, Icon, Button, ListItem…)
└── rmail/   # binário: janela, layout (mock) e estado da UI
```

## Documentação do projeto

- [`docs/PLANEJAMENTO.md`](docs/PLANEJAMENTO.md) — visão, arquitetura, escopo e
  recursos planejados.
- [`AGENTS.md`](AGENTS.md) — regras de desenvolvimento para agentes de IA e humanos.
- [`TODO.md`](TODO.md) — progresso vivo da implementação.

## Licença

MIT.
