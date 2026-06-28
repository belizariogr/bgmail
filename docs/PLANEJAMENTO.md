# Planejamento do rMail

> Documento de direcionamento para humanos **e agentes de IA**. Antes de escrever
> qualquer código, leia também o arquivo [`AGENTS.md`](../AGENTS.md) (regras
> obrigatórias) e o [`TODO.md`](../TODO.md) (estado atual da implementação).

## 1. Visão

O **rMail** é um cliente de e-mail de desktop **rápido** (inicialização quase
instantânea), **simples** e com **design bonito e elegante**. Ele reaproveita a
base de UI do editor [Zed](https://github.com/zed-industries/zed) — o framework
**GPUI** e os padrões de componentes do crate `ui` — para entregar uma interface
nativa, acelerada por GPU e leve.

- **Layout**: igual ao do cliente de e-mail do **macOS** (três colunas + toolbar
  unificada superior + barra de status), porém construído com elementos do Zed.
- **Plataformas**: Windows, Linux e macOS (GPUI é multiplataforma).
- **Linguagem**: Rust.
- **Filosofia**: sem "frescura", mas com acabamento visual impecável.

> Referência local do Zed: `~/dev/zed`. Consulte sempre que precisar entender um
> componente, padrão de layout ou API do GPUI.

## 2. Estratégia de execução (por partes)

1. **Mock visual (etapa atual).** Construir a interface completa com dados
   estáticos para validar o layout e — principalmente — a **velocidade de
   inicialização**. Sem rede, sem persistência, sem lógica de e-mail.
2. **Camada de domínio.** Modelos (`Account`, `Mailbox`, `Message`, `Thread`),
   armazenamento local e máquina de estados de sincronização.
3. **Conectividade.** IMAP/POP3 + SMTP genéricos e, em seguida, **Gmail** (OAuth2).
4. **Recursos de cliente de e-mail.** Ler, compor, responder, encaminhar, mover,
   marcar, buscar, anexos (lista completa na seção 6).
5. **Polimento.** Atalhos de teclado, acessibilidade, animações sutis, empacotamento.

Cada etapa só começa quando a anterior está testada e estável.

## 3. Por que GPUI / Zed

- **Inicialização rápida**: binário nativo, sem runtime web/Electron.
- **Renderização por GPU**: rolagem e redimensionamento fluidos.
- **Estilo "Tailwind em Rust"**: `div().flex().px_3().bg(...)` — produtivo e legível.
- **Sistema de temas** maduro, que espelhamos em um crate `theme` enxuto.

Dependemos do `gpui` **publicado no crates.io** (versão `0.2.2`) para manter o
projeto **reprodutível e portável**, em vez de depender do checkout local do Zed.
O crate local em `~/dev/zed` é usado apenas como **referência de leitura**.

## 4. Arquitetura de crates

O projeto é um *workspace* Cargo. A separação em crates favorece reutilização,
testes isolados e tempos de compilação incremental menores.

| Crate            | Papel                                                                 | Status |
| ---------------- | --------------------------------------------------------------------- | ------ |
| `crates/theme`   | Definição de temas e cores (claro/escuro). Espelha o `theme` do Zed.  | ✅ mock |
| `crates/ui`      | Biblioteca de componentes (`Label`, `Icon`, `Button`, `ListItem`…).   | ✅ mock |
| `crates/rmail`   | Binário: janela, layout, estado da UI (atualmente o mock).            | ✅ mock |
| `crates/mail_core` *(futuro)* | Modelos de domínio e regras de negócio, sem dependência de UI. | ⬜ |
| `crates/storage` *(futuro)*   | Persistência local (SQLite via `sqlez`/`rusqlite`).            | ⬜ |
| `crates/protocols` *(futuro)* | Abstrações IMAP/POP3/SMTP/Gmail atrás de *traits*.            | ⬜ |
| `crates/accounts` *(futuro)*  | Gerência de contas e credenciais (keychain por plataforma).   | ⬜ |

**Regra de dependência:** a UI depende do domínio, nunca o contrário. Domínio e
protocolos não conhecem o GPUI.

### Multiplataforma sem *bloat*

- Toda funcionalidade dependente de SO (keychain, diretórios de dados,
  notificações) fica atrás de uma **trait** com implementações por plataforma
  (`#[cfg(target_os = ...)]`), expostas por uma API única.
- Preferir crates multiplataforma já existentes (`directories`, `keyring`,
  `notify-rust`) a reimplementar. Criar abstração própria **somente** quando
  necessário.
- Minimizar `unsafe`: idealmente **zero** no nosso código. Qualquer uso deve ser
  isolado, comentado com justificativa e coberto por testes.

## 5. Layout da UI (referência: Mail do macOS)

```
┌──────────────────────────────────────────────────────────────────────┐
│  ⦿⦿⦿   [✍ novo] [↻]            [↩][↩↩][↪][🗄][⚑][⌦]   [Tema] [⚙]        │  ← Toolbar (titlebar transparente)
├───────────────┬───────────────────────┬───────────────────────────────┤
│  CONTAS        │  Caixa de entrada   ⌕ │  Assunto da mensagem          │
│  ▾ Pessoal     │ ● GitHub      09:42 📎│  ◉  Remetente                  │
│    ✉ Entrada 7 │   Nova release v0.200 │     remetente@dominio.com  hoje│
│    ✎ Rascunhos │   A nova versão...    │ ───────────────────────────── │
│    ↗ Enviados  │ ● Maria       09:05 ★ │  Corpo da mensagem...          │
│    ⚠ Spam    3 │   Reunião...          │                                │
│  ▾ Trabalho    │   ...                 │                                │
├───────────────┴───────────────────────┴───────────────────────────────┤
│  2 contas · 12 mensagens                    9 não lidas · Atualizado    │  ← Status bar
└──────────────────────────────────────────────────────────────────────┘
```

- **Coluna 1 — Barra lateral** (`panel_background`, ~240px): contas e suas caixas
  (Entrada, Rascunhos, Enviados, Spam, Lixeira, Arquivo), com contadores de não
  lidas.
- **Coluna 2 — Lista de mensagens** (`surface_background`, ~360px): remetente,
  assunto, prévia, horário, indicador de não lida, estrela e clipe de anexo.
- **Coluna 3 — Leitor** (`background`, flexível): cabeçalho (assunto, avatar,
  remetente, data) + corpo.
- **Toolbar** unificada com a barra de título transparente (estilo macOS).
- **Tela de configurações** no estilo do Zed: navegação à esquerda + conteúdo à
  direita (Geral, Contas, Aparência, Notificações).

## 6. Recursos de um cliente de e-mail (escopo)

### MVP (mínimo viável)
- [ ] Conectar contas: **IMAP/POP3** (genérico) e **Gmail** (OAuth2).
- [ ] Listar caixas/pastas por conta.
- [ ] Listar mensagens de uma caixa (com paginação/scroll).
- [ ] Ler mensagem (texto e HTML básico saneado).
- [ ] Marcar como lida/não lida.
- [ ] Compor, **responder**, **responder a todos**, **encaminhar**.
- [ ] Enviar via **SMTP** (e API do Gmail).
- [ ] Mover/excluir/arquivar; marcar como spam.
- [ ] Favoritar (estrela) e sinalizar (flag).
- [ ] Anexos: visualizar lista, baixar, anexar ao compor.
- [ ] Busca (assunto/remetente/corpo).
- [ ] **2 temas**: claro e escuro, com alternância em tempo real.
- [ ] Tela de configurações.

### Pós-MVP (desejável)
- [ ] Múltiplas contas com caixa unificada.
- [ ] Threads/conversas agrupadas.
- [ ] Rascunhos com salvamento automático.
- [ ] Notificações nativas de novos e-mails.
- [ ] Atalhos de teclado configuráveis.
- [ ] Filtros/regras e assinaturas.
- [ ] Modo offline com sincronização.

## 7. Temas

Dois temas embutidos, alternáveis em tempo de execução:

- **Escuro** — paleta baseada em
  [`vscode_dark_modern.zed`](https://github.com/kevcamel/vscode_dark_modern.zed)
  (VSCode *Dark Modern*). Cores-chave: fundo `#1f1f1f`, superfície `#181818`,
  texto `#cccccc`, acento `#0078d4`, seleção `#04395e`.
- **Claro** — paleta baseada em
  [`zed-theme-vscode-light-modern`](https://github.com/XiangpengHao/zed-theme-vscode-light-modern)
  (VSCode *Light Modern*). Cores-chave: fundo `#ffffff`, superfície `#f8f8f8`,
  texto `#3b3b3b`, acento `#005fb8`.

As cores vivem em `crates/theme/src/theme.rs` (`ThemeColors`). Componentes nunca
usam cores literais: usam papéis semânticos via `ui::Color` resolvidos contra o
tema ativo.

## 8. Estratégia de testes

- **Toda função/ação implementada deve ter testes** (regra do projeto).
- Lógica pura (temas, parsing, domínio, protocolos) → testes unitários `#[cfg(test)]`.
- Componentes/visual e fluxos de UI → testes com o `test-support` do GPUI
  (`gpui::TestAppContext`) quando a etapa de lógica começar.
- Rodar sempre: `cargo test --workspace` e `cargo clippy --workspace -- -D warnings`.

## 9. Decisões registradas

- **GPUI via crates.io (`0.2.2`)** em vez de path/git para o checkout local — busca
  reprodutibilidade. (Reavaliar se precisarmos de APIs só presentes no `main`.)
- **Ícones como glifos Unicode no mock**, abstraídos por `IconName`, para evitar o
  pipeline de assets nesta fase. Trocar por SVG (como o Zed) na fase de polimento,
  sem alterar os locais de chamada.
- **Idioma da UI**: Português (Brasil) por padrão (i18n é pós-MVP).

## 10. Como rodar

```bash
cargo run -p rmail        # abre o mock visual
cargo test --workspace    # roda os testes
cargo clippy --workspace  # lint
```
