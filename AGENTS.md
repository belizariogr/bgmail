# AGENTS.md — Regras para agentes de IA (e humanos)

Este arquivo direciona qualquer agente de IA que trabalhe no **rMail**. Leia-o por
completo **antes de escrever ou modificar código**. Em caso de conflito, estas
regras têm prioridade. Consulte também [`docs/PLANEJAMENTO.md`](docs/PLANEJAMENTO.md)
(visão e arquitetura) e [`TODO.md`](TODO.md) (estado atual).

## Regras inegociáveis

1. **Siga sempre estas regras.** Releia este arquivo no início de cada sessão de
   trabalho e mantenha-o em mente em todas as decisões.
2. **Projeto em Rust usando a base de UI do Zed** (framework GPUI + padrões do
   crate `ui`). Não introduza outro framework de UI.
3. **Use o Zed como referência.** O código-fonte está em `~/dev/zed`. Sempre que
   precisar de um componente, padrão de layout ou API do GPUI, **consulte o Zed**
   antes de inventar. Espelhe nomes e padrões dele para facilitar portabilidade.
4. **Melhores práticas e reutilização.** Prefira sempre reaproveitar componentes e
   funções existentes. Extraia abstrações quando houver repetição — mas **sem criar
   bloat**: não adicione camadas, traits ou crates "por precaução".
5. **Crie testes para TODA função/ação implementada.** Nenhuma lógica entra sem
   teste. Rode `cargo test --workspace` antes de considerar uma tarefa concluída.
6. **Multiplataforma (Windows, Linux, macOS).** Todo código específico de SO fica
   atrás de uma abstração (trait + `#[cfg(...)]`) com uma API única e portável.
   Não vaze detalhes de plataforma para a UI ou para o domínio.
7. **Minimize `unsafe`.** Meta: **zero** `unsafe` no nosso código. Se for
   absolutamente necessário, isole-o, comente a justificativa de segurança e
   cubra-o com testes. Justifique no PR/commit.
8. **Mantenha o `TODO.md` atualizado.** Ao iniciar uma tarefa, marque-a em
   andamento; ao concluí-la, marque como feita e adicione os próximos passos
   descobertos. O `TODO.md` é a fonte de verdade do progresso.
9. **Performance é requisito, não detalhe.** O app deve **iniciar rápido**. Evite
   trabalho síncrono pesado na inicialização; carregue dados de forma assíncrona e
   incremental. Meça antes de otimizar.
10. **Design bonito e elegante, porém simples.** Nada de "frescura". Espaçamento,
    alinhamento e hierarquia tipográfica consistentes. Use os papéis de cor
    semânticos do tema, **nunca** cores hexadecimais soltas nos componentes.

## Fluxo de trabalho esperado

1. Leia `AGENTS.md`, `docs/PLANEJAMENTO.md` e `TODO.md`.
2. Escolha/atualize um item no `TODO.md` e marque-o em andamento.
3. Consulte o Zed (`~/dev/zed`) para referências relevantes.
4. Implemente seguindo os padrões existentes do projeto.
5. Escreva os testes correspondentes.
6. Rode `cargo fmt`, `cargo clippy --workspace -- -D warnings` e
   `cargo test --workspace`.
7. Atualize o `TODO.md` (e a documentação, se necessário).
8. Faça commits pequenos e descritivos. **Não** commite segredos/credenciais.

## Convenções de código

- **Edição**: Rust 2021, toolchain fixada em `rust-toolchain.toml`.
- **Formatação**: `cargo fmt` (rustfmt padrão). Sem código não formatado.
- **Lint**: `cargo clippy` sem *warnings* (`-D warnings`).
- **Nomes**: em inglês para identificadores de código; comentários e textos de UI
  podem ser em Português (Brasil), que é o idioma padrão do app.
- **Comentários**: explique o *porquê* (intenção, trade-offs), não o *o quê*. Não
  narre o óbvio.
- **Cores**: defina-as apenas em `crates/theme`; componentes usam `ui::Color`.
- **Componentes**: siga o padrão `#[derive(IntoElement)]` + `impl RenderOnce`, com
  *builder methods* encadeáveis (como no `ui` do Zed).
- **Estado de UI**: views são `Entity` com `impl Render`; mutação via
  `cx.listener(...)` + `cx.notify()`.

## Estrutura do repositório

```
rMail/
├── AGENTS.md            ← você está aqui (regras)
├── README.md           ← visão geral e como rodar
├── TODO.md             ← progresso vivo
├── docs/
│   └── PLANEJAMENTO.md ← visão, arquitetura e escopo
└── crates/
    ├── theme/          ← temas e cores (claro/escuro)
    ├── ui/             ← componentes visuais reutilizáveis
    └── rmail/          ← binário (janela + layout + estado)
```

## Limites de escopo nesta fase (mock)

- **Não** implemente rede, OAuth, IMAP/SMTP ou persistência ainda. A fase atual é
  apenas o **mock visual** (ver `docs/PLANEJAMENTO.md`, seção 2).
- Mantenha os dados de exemplo isolados em `crates/rmail/src/data.rs` para que
  sejam fáceis de substituir pela camada de domínio real depois.
