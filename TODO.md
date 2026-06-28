# TODO — rMail

> Fonte de verdade do progresso. **Mantenha atualizado** (ver regra 8 em
> [`AGENTS.md`](AGENTS.md)). Legenda: ✅ feito · 🔄 em andamento · ⬜ pendente.

## Etapa 0 — Fundação & Planejamento
- ✅ Definir visão, arquitetura e escopo (`docs/PLANEJAMENTO.md`)
- ✅ Escrever regras para agentes (`AGENTS.md`)
- ✅ Criar este `TODO.md`
- ✅ Configurar workspace Cargo + toolchain fixada (`rust-toolchain.toml`)
- ✅ `.gitignore`

## Etapa 1 — Mock visual (atual)
- ✅ Crate `theme`: `ThemeColors`, tema escuro (VSCode Dark Modern) e claro
      (VSCode Light Modern), `ActiveTheme`, toggle + testes
- ✅ Crate `ui`: prelúdio, `Color`, `Label`, `Icon`, `Button`, `IconButton`,
      `ListItem`, helpers `h_flex`/`v_flex`
- ✅ Dados de exemplo (`crates/rmail/src/data.rs`) + testes
- ✅ Layout 3 colunas estilo Mail do macOS (barra lateral, lista, leitor)
- ✅ Toolbar unificada com titlebar transparente + barra de status
- ✅ Alternância de tema claro/escuro em tempo real
- ✅ Tela de configurações estilo Zed (Geral/Contas/Aparência/Notificações)
- ✅ Primeiro build do workspace (`cargo build --workspace`), `cargo clippy`
      sem warnings e `cargo test --workspace` passando; app inicia sem travar
- ✅ macOS/Xcode 26: Metal Toolchain desacoplado — `xcodebuild -downloadComponent
      MetalToolchain` + `.cargo/config.toml` forçando `TOOLCHAINS = "com.apple.dt.toolchain.Metal"`
      (o build script do `gpui` usa `xcrun -sdk macosx metal`, que não acha o stub)
- 🔄 **Medir o tempo de inicialização** com instrumentação (ainda informal)
- ⬜ Testes de UI com `gpui::TestAppContext` (após estabilizar o mock)
- ⬜ Ícones SVG (substituir glifos Unicode) — fase de polimento
- ⬜ Campo de busca funcional (filtra a lista mock)
- ⬜ Tela/painel de composição de e-mail (mock)
- ⬜ Redimensionamento das colunas (divisórias arrastáveis)

## Etapa 2 — Camada de domínio
- ⬜ Crate `mail_core`: `Account`, `Mailbox`, `Message`, `Thread`, `Attachment`
- ⬜ Crate `storage`: persistência local (SQLite) + testes
- ⬜ Máquina de estados de sincronização

## Etapa 3 — Conectividade
- ⬜ Crate `protocols`: traits genéricas (Fetch/Send) + IMAP/POP3/SMTP
- ⬜ Gmail via OAuth2 + API
- ⬜ Crate `accounts`: gerência de contas e credenciais (keychain por plataforma)

## Etapa 4 — Recursos do cliente (ver escopo em PLANEJAMENTO.md §6)
- ⬜ Ler / marcar lida-não lida
- ⬜ Compor / responder / responder a todos / encaminhar
- ⬜ Enviar (SMTP + Gmail)
- ⬜ Mover / excluir / arquivar / spam
- ⬜ Favoritar / sinalizar
- ⬜ Anexos (visualizar, baixar, anexar)
- ⬜ Busca

## Etapa 5 — Polimento
- ⬜ Atalhos de teclado
- ⬜ Acessibilidade
- ⬜ Notificações nativas
- ⬜ Empacotamento (macOS `.app`, Linux, Windows)

## Notas / decisões em aberto
- Reavaliar GPUI crates.io `0.2.2` vs `main` se faltar alguma API.
- Definir formato de armazenamento local (SQLite vs arquivos) na Etapa 2.
