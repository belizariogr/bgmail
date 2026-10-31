# Fidelidade de renderização de e-mail: Blitz × litehtml × Chromium

Gerado por `tools/build_report.py`. Metodologia, ressalvas e como reproduzir em `README.md`.

## Resumo

| | Blitz | litehtml |
|---|---|---|
| Nota média (0–10) | **7.9** | **6.6** |
| Mediana | 8 | 6 |
| E-mails com nota ≥ 8 (usuário não nota problema) | 31/47 | 15/47 |
| E-mails com nota ≥ 6 (legível, parece intencional) | 44/47 | 34/47 |
| E-mails com nota ≤ 4 (quebrado) | 1/47 | 2/47 |
| Vitórias diretas | 34 | 10 (empates: 3) |

### Por origem

| Origem | n | Blitz | litehtml |
|---|---|---|---|
| Cerberus | 2 | 8.5 | 4.0 |
| Gerado por cliente de e-mail | 3 | 8.7 | 9.0 |
| Lee Munroe | 1 | 9.0 | 8.0 |
| MJML | 8 | 8.9 | 8.1 |
| Mailgun | 1 | 0.0 | 9.0 |
| Postmark | 4 | 7.8 | 8.5 |
| react-email (clones de marcas) | 22 | 7.8 | 6.0 |
| react-email (temas Tailwind) | 6 | 7.2 | 4.8 |

## Por e-mail

Colunas: nota (0–10) e SSIM (similaridade estrutural aproximada com o Chromium, 1 = idêntico; serve só de pista: a diferença de fontes pesa bastante).

### `cerberus-hybrid` · Cerberus

Template Cerberus híbrido: hero, colunas 2/3 fluidas, blocos zigue-zague com dir=rtl, imagem de fundo e rodapé escuro/verde.

| | Nota | SSIM | Altura (Chromium 2600px) |
|---|---|---|---|
| Blitz | **8** | 0.687 | 2624px |
| litehtml | **2** | 0.522 | 2448px |

**Blitz:** 
- [médio] Segundo bloco zigue-zague (td dir="rtl") não é invertido: imagem fica à esquerda em vez da direita
- [baixo] Espaçamentos verticais ligeiramente maiores entre blocos (cards e botões um pouco mais altos)
- _CSS suspeito: atributo dir=rtl em td/inline-block_

**litehtml:** 
- [crítico] Layout híbrido de colunas colapsa: colunas 2/3 viram coluna estreita empurrada à direita com grande área branca à esquerda e conteúdo transbordando a borda direita
- [alto] Logo 200x50 ausente e imagem hero 1360x600 deslocada para a direita/cortada
- [alto] Container de 680px ignorado: blocos brancos ocupam/estouram a largura de 800px
- [alto] Texto da faixa com imagem de fundo vira uma linha só que estoura à direita, com placeholder gigante '68…' sobreposto
- [médio] Texto do primeiro bloco e do rodapé sem quebra no container, cortado na borda direita
- _CSS suspeito: display:inline-block + max-width em div de coluna, width:100% em img com max-width, mso/ghost tables_

### `cerberus-responsive` · Cerberus

Template Cerberus responsivo: hero, bloco com background-image, grades de 2 e 3 colunas, cards imagem+texto alternados e rodapé com faixa verde.

| | Nota | SSIM | Altura (Chromium 2468px) |
|---|---|---|---|
| Blitz | **9** | 0.694 | 2494px |
| litehtml | **6** | 0.683 | 2501px |

**Blitz:** 
- [baixo] Espaçamentos verticais ligeiramente maiores entre blocos (botões e cards um pouco mais altos)
- _CSS suspeito: line-height/padding em td_

**litehtml:** 
- [médio] Card 'imagem à direita' (dir=rtl) renderizado com imagem à esquerda, igual ao card anterior
- [médio] Espaçador entre a seção de cards e o bloco de texto seguinte sumiu: os dois cartões brancos ficam colados, separados só por uma linha
- [baixo] Links do rodapé ('View as a Web Page', 'unsubscribe') sem sublinhado
- [baixo] Texto sobre o background-image quebra em 5 linhas em vez de 4 (padding/largura levemente diferentes)
- _CSS suspeito: dir=rtl em td/table; altura de td espaçador (font-size/line-height 0); text-decoration em links_

### `client-apple-mail` · Gerado por cliente de e-mail

Mensagem simples do Apple Mail com imagem inline e citação recuada (blockquote).

| | Nota | SSIM | Altura (Chromium 700px) |
|---|---|---|---|
| Blitz | **10** | 0.959 | 704px |
| litehtml | **9** | 0.717 | 728px |

**Blitz:** nenhum problema relevante.

**litehtml:** 
- [baixo] Espaçamentos verticais um pouco maiores (altura total ~4% maior), sem impacto visual relevante

### `client-gmail-reply` · Gerado por cliente de e-mail

Resposta do Gmail com lista, destaque em amarelo, assinatura em cinza/fonte pequena e citação com borda à esquerda.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **7** | 0.881 | 600px |
| litehtml | **9** | 0.691 | 600px |

**Blitz:** 
- [médio] Assinatura perde a hierarquia: cargo 'Gerente de Projetos · Acme Ltda' sai em preto em vez de cinza e o telefone sai em tamanho normal em vez de fonte pequena
- _CSS suspeito: color/font-size em <font> ou estilo inline herdado na assinatura_

**litehtml:** 
- [baixo] Texto quebra em mais linhas por métricas de fonte maiores, aumentando levemente a altura da citação

### `client-outlook-desktop` · Gerado por cliente de e-mail

E-mail texto estilo Outlook desktop com tabela de indicadores, lista, assinatura e citação de mensagem anterior.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.728 | 600px |
| litehtml | **9** | 0.732 | 600px |

**Blitz:** 
- [baixo] Linhas da tabela de indicadores ligeiramente mais altas e espaçamento vertical um pouco maior
- _CSS suspeito: line-height/mso-line-height em td_

**litehtml:** 
- [baixo] Espaçamento vertical entre blocos ligeiramente maior que no Chromium
- _CSS suspeito: margens de p (MsoNormal)_

### `leemunroe-responsive` · Lee Munroe

E-mail transacional simples com card branco arredondado, botão CTA azul e rodapé centralizado.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.802 | 600px |
| litehtml | **8** | 0.802 | 600px |

**Blitz:** 
- [baixo] Botão e parágrafos com espaçamento vertical um pouco maior que no Chromium

**litehtml:** 
- [baixo] Link 'Unsubscribe' do rodapé sem sublinhado
- [baixo] Botão levemente mais largo e card com padding ligeiramente diferente
- _CSS suspeito: text-decoration em links_

### `mailgun-billing` · Mailgun

Recibo simples em cartão branco centralizado com tabela de itens e total.

| | Nota | SSIM | Altura (Chromium 776px) |
|---|---|---|---|
| Blitz | **0** | 0.881 | 600px |
| litehtml | **9** | 0.855 | 777px |

**Blitz:** 
- [crítico] Render totalmente em branco: apenas o fundo cinza aparece, nenhum conteúdo (cartão, textos e tabela ausentes)
- _CSS suspeito: possível falha de layout/parse no container principal (display:block em td / max-width em tabela)_

**litehtml:** 
- [baixo] Diferenças mínimas de espaçamento; layout praticamente idêntico ao Chromium

### `mjml-austin` · MJML

Newsletter MJML com cabeçalho escuro, hero com imagem de fundo, texto, botões, grade de 2 imagens, mapa e rodapé social.

| | Nota | SSIM | Altura (Chromium 2619px) |
|---|---|---|---|
| Blitz | **8** | 0.705 | 2649px |
| litehtml | **9** | 0.705 | 2653px |

**Blitz:** 
- [médio] Hero com imagem de fundo dividido em dois blocos: emenda escura visível entre o ícone e o título 'Get in person support', com pequeno degrau na borda direita
- [baixo] Espaçamentos verticais ligeiramente maiores (botões e parágrafos), card um pouco mais alto
- _CSS suspeito: background-image/background-size em seções MJML adjacentes (arredondamento de altura)_

**litehtml:** 
- [baixo] Pequenas diferenças de quebra de linha e espaçamento nos parágrafos e no rodapé

### `mjml-black-friday` · MJML

Promoção MJML em fundo preto com logo, menu, imagem hero, título, botão branco e texto legal.

| | Nota | SSIM | Altura (Chromium 950px) |
|---|---|---|---|
| Blitz | **9** | 0.828 | 960px |
| litehtml | **7** | 0.810 | 968px |

**Blitz:** 
- [baixo] Leve aumento de espaçamento vertical entre blocos (botão e rodapé ligeiramente mais abaixo)

**litehtml:** 
- [médio] Botão "Shop Now" alinhado à esquerda em vez de centralizado
- [baixo] Logo ALLURA levemente deslocado para a esquerda
- _CSS suspeito: align/text-align em td de botão MJML (margin auto em tabela)_

### `mjml-newsletter` · MJML

Newsletter MJML com seções coloridas, colunas imagem+texto, screenshot de código e rodapé com ícones sociais.

| | Nota | SSIM | Altura (Chromium 1065px) |
|---|---|---|---|
| Blitz | **9** | 0.901 | 1072px |
| litehtml | **8** | 0.941 | 1067px |

**Blitz:** 
- [baixo] Pequenos acréscimos de espaçamento vertical entre seções

**litehtml:** 
- [baixo] Logo do cabeçalho alinhado à esquerda em vez de centralizado
- _CSS suspeito: align=center / text-align em td com imagem_

### `mjml-racoon` · MJML

Newsletter MJML de e-commerce com hero laranja, grade de produtos em 3 e 2 colunas, blocos lado a lado e rodapé escuro/laranja.

| | Nota | SSIM | Altura (Chromium 2557px) |
|---|---|---|---|
| Blitz | **8** | 0.639 | 2596px |
| litehtml | **9** | 0.661 | 2553px |

**Blitz:** 
- [baixo] Seções de produtos e rodapé de ícones mais altas (espaçamento extra), aumentando a altura total
- [baixo] Títulos do rodapé ("PAYMENT METHODS") quebram em duas linhas
- _CSS suspeito: line-height/padding em colunas mj-column_

**litehtml:** 
- [baixo] Grade de produtos ligeiramente mais compacta; nomes não quebram como no Chromium

### `mjml-real-estate` · MJML

Newsletter MJML imobiliária com hero com imagem de fundo e card sobreposto, seções 2/3 colunas, ícones e CTAs.

| | Nota | SSIM | Altura (Chromium 2241px) |
|---|---|---|---|
| Blitz | **9** | 0.640 | 2279px |
| litehtml | **7** | 0.685 | 2294px |

**Blitz:** 
- [baixo] Seções ligeiramente mais altas (espaçamento vertical maior), sem impacto visual relevante

**litehtml:** 
- [médio] Ícone do caminhão e botão 'Call to action' alinhados à esquerda em vez de centralizados
- [médio] Divisor horizontal antes da seção 'Lorem Ipsum' com imagem alinhado à esquerda em vez da direita
- [médio] Logo do rodapé alinhado à esquerda em vez de centralizado
- [baixo] Link 'Unsubscribe' do rodapé sem sublinhado
- _CSS suspeito: align=center em td/table de imagem e botão (margin:0 auto em tabela)_

### `mjml-recast` · MJML

Newsletter MJML com ilustração no topo, lista de artigos com miniatura à direita, banners e ícones sociais.

| | Nota | SSIM | Altura (Chromium 2414px) |
|---|---|---|---|
| Blitz | **9** | 0.696 | 2429px |
| litehtml | **8** | 0.656 | 2445px |

**Blitz:** 
- [baixo] Espaçamentos verticais ligeiramente maiores entre artigos

**litehtml:** 
- [baixo] Texto do corpo em cinza bem mais claro que no Chromium (contraste menor)
- [baixo] Link 'Recast.AI' sem sublinhado
- [baixo] Títulos dos artigos quebram em mais linhas (coluna de texto um pouco mais estreita)
- _CSS suspeito: color herdada em mj-text/div; text-decoration em links_

### `mjml-ticketshop` · MJML

Promo MJML de venda de ingressos com hero fotográfico, CTA sobreposto, blocos de álbum/banner, logos de parceiros e rodapé amarelo.

| | Nota | SSIM | Altura (Chromium 1350px) |
|---|---|---|---|
| Blitz | **9** | 0.758 | 1370px |
| litehtml | **7** | 0.766 | 1359px |

**Blitz:** 
- [baixo] Pequenas diferenças de espaçamento vertical (≈1,5% mais alto)

**litehtml:** 
- [médio] Botão 'BUY TICKETS' no hero alinhado à esquerda em vez de centralizado
- [médio] Botão 'OUR PARTNERS' alinhado à esquerda em vez de centralizado
- _CSS suspeito: align=center em td de botão MJML (tabela com margin auto)_

### `mjml-welcome-email` · MJML

Boas-vindas MJML com imagem de cabeçalho, bloco azul com título, texto e botão 'Login' arredondado.

| | Nota | SSIM | Altura (Chromium 630px) |
|---|---|---|---|
| Blitz | **10** | 0.880 | 637px |
| litehtml | **10** | 0.872 | 634px |

**Blitz:** nenhum problema relevante.

**litehtml:** nenhum problema relevante.

### `postmark-comment-notification` · Postmark

Notificação de comentário com bloco de anexos cinza, links e rodapé de endereço.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **8** | 0.836 | 600px |
| litehtml | **9** | 0.835 | 600px |

**Blitz:** 
- [baixo] Corpo branco com margens laterais (não ocupa a largura toda) e espaçamentos verticais um pouco maiores
- _CSS suspeito: width:100% em tabela com padding/margin_

**litehtml:** 
- [baixo] Bloco de anexos levemente mais largo e espaçamento vertical ligeiramente diferente

### `postmark-invoice` · Postmark

Fatura transacional Postmark com caixa de valor, botão verde, tabela de itens e rodapé cinza.

| | Nota | SSIM | Altura (Chromium 1127px) |
|---|---|---|---|
| Blitz | **8** | 0.799 | 1195px |
| litehtml | **9** | 0.819 | 1152px |

**Blitz:** 
- [baixo] Corpo branco com margem/borda cinza lateral visível em vez de ocupar a largura toda
- [baixo] Espaçamento vertical maior na tabela de itens e entre blocos
- _CSS suspeito: width:100% em tabela body com padding/margin_

**litehtml:** 
- [baixo] Espaçamentos verticais um pouco maiores na tabela de itens

### `postmark-receipt` · Postmark

Recibo Postmark com cupom em caixa tracejada, tabela de itens, botões CTA e rodapé cinza.

| | Nota | SSIM | Altura (Chromium 1328px) |
|---|---|---|---|
| Blitz | **7** | 0.701 | 1398px |
| litehtml | **9** | 0.746 | 1356px |

**Blitz:** 
- [médio] Corpo branco não ocupa a largura toda: aparece uma faixa cinza nas laterais do conteúdo
- [baixo] Padding das células da tabela de itens e espaçamentos verticais maiores (altura ~5% maior)
- _CSS suspeito: width:100% em tabela aninhada / padding de td_

**litehtml:** 
- [baixo] Linha divisória acima do rodapé interno um pouco mais curta que a largura do conteúdo

### `postmark-welcome` · Postmark

E-mail transacional Postmark de boas-vindas com botão CTA, caixas de atributos cinza, links e rodapé.

| | Nota | SSIM | Altura (Chromium 1179px) |
|---|---|---|---|
| Blitz | **8** | 0.701 | 1235px |
| litehtml | **7** | 0.726 | 1247px |

**Blitz:** 
- [baixo] Card de conteúdo branco ganha margem lateral cinza e borda visível; espaçamentos ~5% maiores
- _CSS suspeito: width/padding em .email-body / body_action_

**litehtml:** 
- [médio] Botão 'Do this Next' estreito, com texto quebrado em duas linhas
- [baixo] Links ('email our customer success team', 'live chat', 'help documentation') sem sublinhado
- _CSS suspeito: white-space/largura intrínseca de a.button (display:inline-block); text-decoration em a_

### `re-02-matte-welcome` · react-email (temas Tailwind)

Boas-vindas com card central arredondado, imagem hero, checklist com setas e seção final com colagem de imagens e rodapé.

| | Nota | SSIM | Altura (Chromium 2427px) |
|---|---|---|---|
| Blitz | **7** | 0.722 | 2469px |
| litehtml | **5** | 0.671 | 2481px |

**Blitz:** 
- [médio] Imagem hero com recuo interno (não encosta nas bordas do card)
- [médio] Espaçamentos verticais maiores na checklist e na seção 'Try Composer'
- [baixo] Texto do rodapé ('marketing emails.') colado na borda inferior do card
- _CSS suspeito: padding em td / line-height_

**litehtml:** 
- [alto] Largura máxima do card ignorada: container ~500px em vez de ~400px (escala)
- [alto] Imagens hero e da colagem não acompanham a largura do card, deixando faixa vazia à direita
- [médio] Sem border-radius e sombra no card
- _CSS suspeito: max-width em container; width:100% em img; border-radius; box-shadow_

### `re-03-protocol-product-update` · react-email (temas Tailwind)

Atualização de produto em tema escuro (React Email) com imagens, títulos grandes, seção de blog em 2 colunas e rodapé.

| | Nota | SSIM | Altura (Chromium 2729px) |
|---|---|---|---|
| Blitz | **9** | 0.605 | 2771px |
| litehtml | **5** | 0.508 | 2680px |

**Blitz:** 
- [baixo] Espaçamentos verticais ligeiramente maiores, altura total um pouco maior

**litehtml:** 
- [alto] max-width do container ignorado: conteúdo ocupa os 800px inteiros em vez da coluna central estreita
- [médio] Imagens não acompanham a largura do container (ficam menores que a coluna, sobra espaço à direita)
- [baixo] Links "Explore Smart Tasks"/"Read more" renderizados em fonte monoespaçada
- _CSS suspeito: max-width em tabela/div container; width:100% em img_

### `re-04-arcane-newsletter` · react-email (temas Tailwind)

Newsletter escura (React Email) com container de 600px, imagens full-bleed, lista numerada, depoimento e rodapé.

| | Nota | SSIM | Altura (Chromium 3661px) |
|---|---|---|---|
| Blitz | **7** | 0.640 | 3838px |
| litehtml | **4** | 0.464 | 3587px |

**Blitz:** 
- [médio] Imagens hero e do produto aparecem recuadas com uma borda clara em vez de sangrar até a borda do container
- [baixo] Link 'See what's new' com fonte maior que no Chromium
- [baixo] Espaçamentos verticais maiores (altura ~5% maior) e pequena faixa branca no topo
- _CSS suspeito: padding/border em img ou td de imagem; font-size em link_

**litehtml:** 
- [alto] Container de 600px ignorado: fundo escuro e conteúdo ocupam os 800px inteiros
- [alto] Imagens não preenchem a largura: faixa de cor diferente à direita da hero e da imagem do produto
- [médio] Texto e colunas da lista numerada esticados pela largura total, mudando a composição
- _CSS suspeito: max-width em tabela container; width:100% em img_

### `re-04-arcane-order-confirmation` · react-email (temas Tailwind)

Confirmação de pedido com fundo bordô, título serifado grande, itens com miniatura, tabela de totais e rodapé com ícones.

| | Nota | SSIM | Altura (Chromium 1472px) |
|---|---|---|---|
| Blitz | **8** | 0.748 | 1545px |
| litehtml | **5** | 0.543 | 1598px |

**Blitz:** 
- [baixo] Paddings verticais maiores (linhas da tabela de totais e itens mais altas; altura total ~5% maior)
- [baixo] Pequena margem branca no topo antes do container bordô
- _CSS suspeito: padding/line-height em td_

**litehtml:** 
- [alto] Largura máxima do container ignorada: fundo bordô ocupa os 800px inteiros, sem as margens brancas laterais
- [médio] Tabela de totais e divisores se estendem por toda a largura, desalinhados do texto
- [baixo] Título 'Your order has been placed' quebra em 3 linhas em vez de 2
- _CSS suspeito: max-width em tabela/container_

### `re-05-studio-abandoned-cart` · react-email (temas Tailwind)

Carrinho abandonado com bloco cinza central, card de produto, depoimento, três colunas de benefícios e rodapé centralizado.

| | Nota | SSIM | Altura (Chromium 1644px) |
|---|---|---|---|
| Blitz | **6** | 0.834 | 1756px |
| litehtml | **5** | 0.834 | 1654px |

**Blitz:** 
- [médio] Botões 'Return to cart' alinhados à esquerda em vez de centralizados
- [médio] Título 'Don't take it from us' e textos do rodapé alinhados à esquerda em vez de centralizados
- [baixo] Espaçamentos verticais maiores (altura ~7% maior) com área branca extra no final
- _CSS suspeito: text-align:center herdado / margin:auto em tabela de botão_

**litehtml:** 
- [alto] Container de 600px ignorado: bloco cinza ocupa os 800px inteiros
- [médio] Botões, título 'Don't take it from us' e rodapé alinhados à esquerda em vez de centralizados
- [baixo] Parágrafo de introdução renderizado em fonte monoespaçada e título em serifa, quebrando a hierarquia tipográfica
- _CSS suspeito: max-width em tabela; text-align/margin:auto; font-family com lista de fallback_

### `re-05-studio-promo` · react-email (temas Tailwind)

Promo Black Friday (react-email) com barra superior, hero com gradiente e anel, depoimento em card, 3 colunas de benefícios e rodapé com ícones sociais.

| | Nota | SSIM | Altura (Chromium 2253px) |
|---|---|---|---|
| Blitz | **6** | 0.860 | 2337px |
| litehtml | **5** | 0.852 | 2426px |

**Blitz:** 
- [médio] Hero com gradiente dividido em dois blocos com borda/sombra visíveis (imagem e CTA), em vez de um fundo contínuo sem borda
- [médio] Texto do rodapé (descrição, endereço, unsubscribe) alinhado à esquerda em vez de centralizado
- [baixo] Faixa escura do topo ligeiramente mais larga que o container
- _CSS suspeito: background linear-gradient em td/container, border/box-shadow, text-align em rodapé_

**litehtml:** 
- [alto] Container de ~600px ignorado: hero, depoimento e rodapé ocupam os 800px inteiros
- [médio] Imagem do anel maior e CTA 'Save up to $80' deslocado para baixo, alongando o hero
- [médio] Rodapé (descrição, ícones sociais, endereço, unsubscribe) alinhado à esquerda em vez de centralizado
- [baixo] Texto da barra superior renderizado em fonte monoespaçada
- _CSS suspeito: max-width em table/div container, text-align/margin auto no rodapé_

### `re-airbnb-review` · react-email (clones de marcas)

Aviso de avaliação com avatar circular, citação em caixa cinza, botão CTA e links de perguntas frequentes.

| | Nota | SSIM | Altura (Chromium 989px) |
|---|---|---|---|
| Blitz | **8** | 0.691 | 1036px |
| litehtml | **7** | 0.753 | 1000px |

**Blitz:** 
- [baixo] Espaçamentos verticais um pouco maiores (~5% de altura extra)
- [baixo] Container ligeiramente mais largo à direita

**litehtml:** 
- [médio] Avatar renderizado quadrado em vez de circular
- [baixo] Link 'Report unsafe behavior' sem sublinhado
- _CSS suspeito: border-radius em img_

### `re-amazon-review` · react-email (clones de marcas)

Pedido de avaliação da Amazon com capa de livro, estrelas, faixa CTA azul e rodapé com ícones sociais.

| | Nota | SSIM | Altura (Chromium 935px) |
|---|---|---|---|
| Blitz | **6** | 0.542 | 1022px |
| litehtml | **6** | 0.566 | 935px |

**Blitz:** 
- [médio] Borda laranja desenhada em volta de todo o container em vez de apenas no topo
- [baixo] Coluna de texto ao lado da capa mais estreita, quebrando mais linhas; altura total maior
- _CSS suspeito: border-top / border shorthand com border-width zero_

**litehtml:** 
- [médio] max-width do container ignorado: conteúdo se expande até ~800px
- [baixo] Link "View more" sem sublinhado
- _CSS suspeito: max-width em tabela container_

### `re-apple-receipt` · react-email (clones de marcas)

Recibo da Apple com tabelas aninhadas de dados, linha de item com ícone, total, bloco promocional Apple Card e rodapé longo.

| | Nota | SSIM | Altura (Chromium 1398px) |
|---|---|---|---|
| Blitz | **5** | 0.666 | 1469px |
| litehtml | **6** | 0.692 | 1505px |

**Blitz:** 
- [alto] Bloco promocional sem o logo 'Card' e sem o título 'Save 3% on all your Apple purchases.' — só aparece o link 'Apply and use in minutes'
- [médio] Tabela de dados (Apple ID/Billed to) com células centralizadas verticalmente e padding excessivo, desalinhando as colunas
- [baixo] Linha do total e seções com espaçamentos maiores; separador entre 'Write a Review' e 'Report a Problem' sem espaçamento
- _CSS suspeito: vertical-align em td; display/height em elementos do bloco Apple Card_

**litehtml:** 
- [médio] Linha de TOTAL quebrada: rótulo deslocado à esquerda e borda vertical se estende para baixo da tabela
- [médio] Logo 'Card' alinhado à esquerda em vez de centralizado acima do título
- [baixo] Espaçamentos verticais maiores (altura ~8% maior)
- _CSS suspeito: border em td com rowspan/height; text-align em td_

### `re-aws-verify-email` · react-email (clones de marcas)

E-mail AWS de código de verificação: cabeçalho escuro com logo, card branco com código grande e rodapé legal.

| | Nota | SSIM | Altura (Chromium 665px) |
|---|---|---|---|
| Blitz | **9** | 0.639 | 718px |
| litehtml | **6** | 0.639 | 672px |

**Blitz:** 
- [baixo] Espaçamento vertical ~8% maior (cabeçalho e parágrafos mais altos)

**litehtml:** 
- [alto] Container de 600px ignorado: card ocupa toda a largura e encosta/estoura a borda direita
- [médio] Logo AWS alinhado à esquerda em vez de centralizado no cabeçalho
- _CSS suspeito: max-width em table container; align=center em td/img_

### `re-codepen-challengers` · react-email (clones de marcas)

Newsletter CodePen com banner, caixas amarelas, imagem grande, bloco PRO, grade de cards em 2 colunas e botão CTA.

| | Nota | SSIM | Altura (Chromium 3156px) |
|---|---|---|---|
| Blitz | **7** | 0.471 | 3241px |
| litehtml | **8** | 0.634 | 3200px |

**Blitz:** 
- [médio] Emojis inline ausentes (cubo de gelo, mão, braço, óculos escuros): deixam lacunas no texto
- [médio] Cards de IDEAS/RESOURCES com padding interno muito maior e texto deslocado para baixo, ficando bem mais altos
- [baixo] Espaçamentos verticais gerais um pouco maiores
- _CSS suspeito: fallback de fonte emoji; padding/vertical-align em td dos cards_

**litehtml:** 
- [baixo] Links do rodapé ('email preferences', 'instantly opt out', 'support') sem sublinhado
- [baixo] Textos quebram um pouco diferente, cards levemente mais altos
- _CSS suspeito: text-decoration em links_

### `re-dropbox-reset-password` · react-email (clones de marcas)

Redefinição de senha Dropbox: card branco centralizado com logo, texto e botão azul.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.743 | 600px |
| litehtml | **6** | 0.798 | 600px |

**Blitz:** 
- [baixo] Card levemente mais largo/alto e deslocado alguns pixels

**litehtml:** 
- [médio] Largura do card ignorada: ocupa quase os 800px em vez de ~600px
- [baixo] Texto em cinza mais claro que no Chromium
- [baixo] Link 'more security tips' sem sublinhado
- _CSS suspeito: max-width/width em tabela_

### `re-github-access-token` · react-email (clones de marcas)

Notificação curta com título, caixa com borda contendo texto e botão verde, e rodapé.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.816 | 600px |
| litehtml | **5** | 0.767 | 600px |

**Blitz:** 
- [baixo] Pequenas diferenças de espaçamento vertical

**litehtml:** 
- [alto] Largura máxima ignorada: conteúdo e caixa com borda ocupam os 800px inteiros
- [médio] Título em uma única linha e caixa achatada por causa da largura errada
- _CSS suspeito: max-width em container_

### `re-google-play-policy-update` · react-email (clones de marcas)

Comunicado de política do Google Play em cartão branco com faixas gráficas azuis, texto e rodapé "Connect with us".

| | Nota | SSIM | Altura (Chromium 980px) |
|---|---|---|---|
| Blitz | **7** | 0.604 | 1019px |
| litehtml | **6** | 0.500 | 905px |

**Blitz:** 
- [médio] Bloco "Connect with us" deslocado para a esquerda e mais largo, desalinhado com o conteúdo do cartão
- [baixo] Parágrafos encostam na borda direita do cartão (padding direito parcialmente ignorado)
- _CSS suspeito: padding/width em td aninhada; margin negativa_

**litehtml:** 
- [médio] Cartão ocupa quase todos os 800px: largura máxima do container ignorada
- [baixo] Faixa decorativa do rodapé sobreposta aos ícones de forma diferente
- _CSS suspeito: max-width em tabela container_

### `re-koala-welcome` · react-email (clones de marcas)

Boas-vindas simples com logo, parágrafo, botão de largura total do container e rodapé.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.826 | 600px |
| litehtml | **6** | 0.869 | 600px |

**Blitz:** 
- [baixo] Container ligeiramente mais largo e espaçamentos um pouco maiores

**litehtml:** 
- [alto] Container de ~580px ignorado: texto, botão e divisória ocupam os 800px inteiros
- _CSS suspeito: max-width em container_

### `re-linear-login-code` · react-email (clones de marcas)

E-mail Linear com código de login: logo, título, botão largo, texto e código em bloco monoespaçado.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.842 | 600px |
| litehtml | **6** | 0.874 | 600px |

**Blitz:** 
- [baixo] Espaçamentos verticais ligeiramente maiores

**litehtml:** 
- [alto] Container de ~560px ignorado: botão 'Login to Linear' e divisor ocupam quase toda a largura de 800px
- _CSS suspeito: max-width em container (div/table)_

### `re-netlify-welcome` · react-email (clones de marcas)

Boas-vindas Netlify com logo, card branco com lista de passos, botão CTA, links de navegação e rodapé.

| | Nota | SSIM | Altura (Chromium 974px) |
|---|---|---|---|
| Blitz | **9** | 0.653 | 1030px |
| litehtml | **6** | 0.674 | 919px |

**Blitz:** 
- [baixo] Espaçamentos verticais um pouco maiores (altura ~6% maior)

**litehtml:** 
- [médio] Largura máxima do card ignorada: conteúdo ocupa quase os 800px
- [médio] Rodapé ('Unsubscribe', 'Manage Preferences') alinhado à esquerda e dentro da área branca, em vez de centralizado sobre o fundo cinza
- [baixo] Links 'Visit the forums', 'Read the docs', 'Contact an expert' sem sublinhado
- _CSS suspeito: max-width em tabela/div; text-align em td do rodapé_

### `re-nike-receipt` · react-email (clones de marcas)

Recibo de envio com rastreio, produto, dados do pedido, grade de 4 recomendações e rodapé com links.

| | Nota | SSIM | Altura (Chromium 2169px) |
|---|---|---|---|
| Blitz | **7** | 0.743 | 2317px |
| litehtml | **6** | 0.756 | 2234px |

**Blitz:** 
- [médio] Altura total ~7% maior por espaçamentos verticais extras entre seções
- [baixo] Colunas Order Number/Order Date deslocadas para a direita
- [baixo] Links do rodapé (Men/Women/Kids/Customize) com espaçamento diferente
- _CSS suspeito: padding/line-height em td_

**litehtml:** 
- [médio] Botão 'Track Package' desalinhado: colado à esquerda junto ao número de rastreio em vez de alinhado à direita
- [médio] Links 'Men Women Kids' amontoados sem espaçamento, 'Customize' separado
- [médio] Telefone e horário de atendimento juntos na mesma célula sem separação
- [baixo] Botão 'Order Status' alinhado à esquerda em vez de centralizado
- _CSS suspeito: width em td / align em tabela; display:inline-block_

### `re-notion-magic-link` · react-email (clones de marcas)

E-mail de login com link mágico, bloco de código cinza e assinatura com logo.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.831 | 600px |
| litehtml | **7** | 0.833 | 600px |

**Blitz:** 
- [baixo] Espaçamentos verticais ligeiramente maiores

**litehtml:** 
- [médio] Bloco do código de login se estende quase até a borda direita (largura máxima ignorada)
- _CSS suspeito: max-width em div/tabela_

### `re-papermark-year-in-review` · react-email (clones de marcas)

Retrospectiva anual Papermark com cartões arredondados com gradientes, estatísticas em 3 colunas, pílulas de países e botões escuros.

| | Nota | SSIM | Altura (Chromium 2235px) |
|---|---|---|---|
| Blitz | **9** | 0.695 | 2290px |
| litehtml | **5** | 0.783 | 2243px |

**Blitz:** 
- [baixo] Espaçamento vertical ligeiramente maior nos cartões

**litehtml:** 
- [alto] max-width do container ignorado: cartões ocupam os 800px inteiros
- [médio] Gradientes de fundo dos cartões perdidos ou muito atenuados (cinza e verde ficam chapados)
- [baixo] Pílulas de países alinhadas à esquerda em vez de centralizadas
- _CSS suspeito: max-width; linear-gradient em background_

### `re-plaid-verify-identity` · react-email (clones de marcas)

E-mail de código de verificação Plaid com cartão estreito centralizado, ícone, código grande e rodapé.

| | Nota | SSIM | Altura (Chromium 600px) |
|---|---|---|---|
| Blitz | **9** | 0.882 | 610px |
| litehtml | **5** | 0.870 | 600px |

**Blitz:** 
- [baixo] Cartão um pouco mais alto que no Chromium

**litehtml:** 
- [alto] Cartão estreito (~360px) ignorado: ocupa os 800px inteiros
- [médio] Título "Enter the following code..." alinhado à esquerda em vez de centralizado
- _CSS suspeito: max-width/width em tabela; text-align herdado_

### `re-raycast-magic-link` · react-email (clones de marcas)

Magic link com fundo em gradiente, título com emoji, link destacado e rodapé com logo em cinza.

| | Nota | SSIM | Altura (Chromium 614px) |
|---|---|---|---|
| Blitz | **7** | 0.885 | 634px |
| litehtml | **6** | 0.855 | 619px |

**Blitz:** 
- [médio] Emojis ausentes (varinha no título e mãos apontando no link), deixando espaços vazios
- [baixo] Logo do rodapé sai colorido em vez de acinzentado
- _CSS suspeito: fallback de fonte de emoji; filter:grayscale/opacity em img_

**litehtml:** 
- [alto] Container de 600px ignorado: card com gradiente ocupa os 800px inteiros
- [baixo] Logo do rodapé sai colorido em vez de acinzentado
- _CSS suspeito: max-width em container; filter:grayscale em img_

### `re-slack-confirm` · react-email (clones de marcas)

E-mail Slack de confirmação com código em caixa cinza, rodapé com logo, ícones sociais e links.

| | Nota | SSIM | Altura (Chromium 674px) |
|---|---|---|---|
| Blitz | **9** | 0.730 | 715px |
| litehtml | **6** | 0.755 | 686px |

**Blitz:** 
- [baixo] Espaçamento vertical ~6% maior (caixa do código e rodapé mais altos)

**litehtml:** 
- [alto] Container de ~560px ignorado: caixa do código e conteúdo ocupam quase toda a largura de 800px
- [baixo] Links do rodapé ('Our blog', 'Policies'…) sem sublinhado
- _CSS suspeito: max-width em container; text-decoration em a_

### `re-stack-overflow-tips` · react-email (clones de marcas)

Dicas Stack Overflow: cabeçalho com logo, hero azul com ilustração, seções de texto com lista, botão CTA e rodapé.

| | Nota | SSIM | Altura (Chromium 1149px) |
|---|---|---|---|
| Blitz | **5** | 0.575 | 1213px |
| litehtml | **8** | 0.664 | 1221px |

**Blitz:** 
- [alto] Botão 'I need a break' perdeu padding/fundo: aparece como texto inline com realce azul, não como botão
- [médio] Coluna de texto do hero mais estreita: título e subtítulo quebram em mais linhas e o hero fica mais alto
- [baixo] Caixa '<3' do rodapé esticada por toda a largura
- [baixo] Borda clara visível ao redor da área do logo no topo
- _CSS suspeito: display:inline-block/padding em <a> de botão; width em td do hero_

**litehtml:** 
- [baixo] Caixa '<3' do rodapé esticada por toda a largura
- [baixo] Links do rodapé sem sublinhado
- [baixo] Itens da lista com espaçamento vertical maior
- _CSS suspeito: display:inline-block; text-decoration em links_

### `re-stripe-welcome` · react-email (clones de marcas)

Boas-vindas Stripe em card branco centralizado com botão largo, parágrafos com links e rodapé.

| | Nota | SSIM | Altura (Chromium 819px) |
|---|---|---|---|
| Blitz | **8** | 0.629 | 883px |
| litehtml | **5** | 0.655 | 800px |

**Blitz:** 
- [baixo] Altura ~8% maior por espaçamentos e quebras de linha diferentes
- [baixo] Card um pouco mais baixo e com mais espaço inferior
- _CSS suspeito: line-height_

**litehtml:** 
- [alto] Largura máxima do card ignorada: card ocupa ~780px em vez de ~600px
- [médio] Botão CTA e parágrafos esticados acompanhando a largura errada
- _CSS suspeito: max-width em container_

### `re-twitch-reset-password` · react-email (clones de marcas)

Aviso de troca de senha da Twitch em cartão branco sobre fundo cinza com logo, texto e rodapé com ícones.

| | Nota | SSIM | Altura (Chromium 612px) |
|---|---|---|---|
| Blitz | **7** | 0.684 | 660px |
| litehtml | **5** | 0.636 | 603px |

**Blitz:** 
- [médio] Texto do corpo ultrapassa a borda direita do cartão branco (padding/largura incorretos)
- [baixo] Espaçamento vertical maior entre cabeçalho e corpo
- _CSS suspeito: padding em td com width fixa (box-sizing)_

**litehtml:** 
- [alto] Cartão de 600px ignorado: conteúdo ocupa os 800px inteiros
- [médio] Ícones sociais do rodapé separados (Twitter à esquerda, Facebook ao centro) em vez de agrupados e centralizados
- _CSS suspeito: max-width em tabela; display:inline-block/width em td de ícones_

### `re-vercel-invite-user` · react-email (clones de marcas)

Convite Vercel em card com borda, avatares circulares ligados por seta, botão preto e rodapé de segurança.

| | Nota | SSIM | Altura (Chromium 739px) |
|---|---|---|---|
| Blitz | **8** | 0.792 | 773px |
| litehtml | **5** | 0.776 | 703px |

**Blitz:** 
- [baixo] Avatares e seta mais espaçados entre si; espaçamentos verticais um pouco maiores
- _CSS suspeito: width em td de colunas_

**litehtml:** 
- [alto] Container de ~465px ignorado: card ocupa os 800px inteiros
- [médio] Avatares alinhados à esquerda em vez de centralizados e sem border-radius (quadrados)
- [baixo] Texto do botão 'Join the team' em fonte serifada
- _CSS suspeito: max-width em container; border-radius em img; align em td_

### `re-yelp-recent-login` · react-email (clones de marcas)

Alerta Yelp de login recente com card bordado, ilustração no topo, dados de sessão, botão vermelho e rodapé ilustrado.

| | Nota | SSIM | Altura (Chromium 1013px) |
|---|---|---|---|
| Blitz | **7** | 0.647 | 1081px |
| litehtml | **6** | 0.657 | 983px |

**Blitz:** 
- [médio] Container não centralizado: card e logo deslocados para a esquerda e card um pouco mais largo
- [baixo] Título 'We noticed a recent login…' e espaçamentos maiores, alongando o card (~7% mais alto)
- _CSS suspeito: margin:0 auto / align=center em table container com max-width_

**litehtml:** 
- [alto] Container de 600px ignorado: card fica mais largo que no Chromium
- [médio] Ilustração do topo não preenche a largura do card, deixando faixa branca à direita
- _CSS suspeito: max-width em table; width:100% em img_

## Adendo: litehtml com 3 correções no motor

Patch: `patches/litehtml-table-maxwidth-align-intrinsic.patch` (+48 linhas de C++ em 3 arquivos):
1. `render_table.cpp`: `max-width` limita a largura usada de `<table>`.
2. `el_table.cpp`: `align=center|left|right` na tabela (margens auto/float) e centralização de tabelas filhas de `align=center`/`<center>` (equivalente ao `-webkit-center` dos navegadores).
3. `render_block.cpp`: a largura intrínseca devolvida respeita `max-width` (colunas "hybrid" `width:100%; max-width:220px` inflavam a tabela; era isso, e não `inline-block` em si, que quebrava o Cerberus).

Os 28 e-mails cuja renderização mudou foram re-revisados com a mesma rubrica; nenhuma regressão (os outros 19 ficaram pixel a pixel iguais).

| | Blitz | litehtml original | litehtml corrigido |
|---|---|---|---|
| Nota média | 7.9 | 6.6 | 8.1 |
| Mediana | 8 | 6 | 8 |
| Nota ≥ 8 | 31/47 | 15/47 | 35/47 |
| Nota ≥ 6 | 44/47 | 34/47 | 46/47 |
| Nota ≤ 4 | 1/47 | 2/47 | 0/47 |

Problemas restantes mais comuns no litehtml corrigido: links sem sublinhado e fontes genéricas (`monospace`/`serif`) caindo em fonte errada, ambos limitações do **pintor de exemplo** do litehtml-rs (não do layout); alguns centralizações por `text-align`/`margin:auto` em `div`; `box-shadow` (não suportado) e `border-radius` em `<img>`.

Todos os 47 e-mails (os 19 sem mudança repetem a nota anterior):

| E-mail | Blitz | litehtml antes | litehtml depois | Problemas restantes (depois) |
|---|---|---|---|---|
| `cerberus-hybrid` | 8 | 2 | **6** | [médio] Linhas com imagem 200x200 + texto (esquerda/direita) empilham verticalmente em vez de ficarem lado a lado, alongando o e-mail; [médio] Espaçamento escuro entre o card da grade e o bloco de texto seguinte desaparece: blocos brancos ficam colados, separados só por uma linha; [baixo] Links do rodapé ('View as a Web Page', 'unsubscribe') sem sublinhado |
| `cerberus-responsive` | 9 | 6 | **6** | [médio] Card 'imagem à direita' (dir=rtl) renderizado com imagem à esquerda, igual ao card anterior; [médio] Espaçador entre a seção de cards e o bloco de texto seguinte sumiu: os dois cartões brancos ficam colados, separados só por uma linha; [baixo] Links do rodapé ('View as a Web Page', 'unsubscribe') sem sublinhado; [baixo] Texto sobre o background-image quebra em 5 linhas em vez de 4 (padding/largura levemente diferentes) |
| `client-apple-mail` | 10 | 9 | **9** | [baixo] Espaçamentos verticais um pouco maiores (altura total ~4% maior), sem impacto visual relevante |
| `client-gmail-reply` | 7 | 9 | **9** | [baixo] Texto quebra em mais linhas por métricas de fonte maiores, aumentando levemente a altura da citação |
| `client-outlook-desktop` | 9 | 9 | **9** | [baixo] Espaçamento vertical entre blocos ligeiramente maior que no Chromium |
| `leemunroe-responsive` | 9 | 8 | **8** | [baixo] Link 'Unsubscribe' do rodapé sem sublinhado; [baixo] Botão levemente mais largo e card com padding ligeiramente diferente |
| `mailgun-billing` | 0 | 9 | **9** | [baixo] Diferenças mínimas de espaçamento; layout praticamente idêntico ao Chromium |
| `mjml-austin` | 8 | 9 | **9** | [baixo] Card principal sem sombra (box-shadow) em volta; [baixo] Links no texto e links 'Privacy'/'Unsubscribe' do rodapé sem sublinhado; [baixo] Pequenas diferenças de quebra de linha e espaçamento nos parágrafos e no rodapé |
| `mjml-black-friday` | 9 | 7 | **9** | — |
| `mjml-newsletter` | 9 | 8 | **8** | [baixo] Logo do cabeçalho alinhado à esquerda em vez de centralizado |
| `mjml-racoon` | 8 | 9 | **9** | [baixo] Grade de produtos ligeiramente mais compacta; nomes não quebram como no Chromium |
| `mjml-real-estate` | 9 | 7 | **8** | [médio] Divisor horizontal antes da seção 'Lorem Ipsum' com imagem alinhado à esquerda em vez da direita; [baixo] Espaço vertical extra antes do bloco 'Window House 23'; [baixo] Link 'Unsubscribe' do rodapé sem sublinhado |
| `mjml-recast` | 9 | 8 | **8** | [baixo] Texto do corpo em cinza bem mais claro que no Chromium (contraste menor); [baixo] Link 'Recast.AI' sem sublinhado; [baixo] Títulos dos artigos quebram em mais linhas (coluna de texto um pouco mais estreita) |
| `mjml-ticketshop` | 9 | 7 | **9** | — |
| `mjml-welcome-email` | 10 | 10 | **10** | — |
| `postmark-comment-notification` | 8 | 9 | **9** | [baixo] Bloco de anexos levemente mais largo e espaçamento vertical ligeiramente diferente |
| `postmark-invoice` | 8 | 9 | **9** | [baixo] Espaçamentos verticais um pouco maiores na tabela de itens |
| `postmark-receipt` | 7 | 9 | **9** | [baixo] Linha divisória acima do rodapé interno um pouco mais curta que a largura do conteúdo |
| `postmark-welcome` | 8 | 7 | **7** | [médio] Botão 'Do this Next' estreito, com texto quebrado em duas linhas; [baixo] Links ('email our customer success team', 'live chat', 'help documentation') sem sublinhado |
| `re-02-matte-welcome` | 7 | 5 | **8** | [médio] Sem border-radius e sombra no card |
| `re-03-protocol-product-update` | 9 | 5 | **9** | [baixo] Links "Explore Smart Tasks"/"Read more" renderizados em fonte monoespaçada |
| `re-04-arcane-newsletter` | 7 | 4 | **9** | [baixo] E-mail ~3% mais alto: título 'Tips And Resources', citação e lista numerada quebram em mais linhas |
| `re-04-arcane-order-confirmation` | 8 | 5 | **8** | [baixo] Título 'Your order has been placed' quebra em 3 linhas em vez de 2, aumentando a altura total |
| `re-05-studio-abandoned-cart` | 6 | 5 | **6** | [médio] Botões 'Return to cart' e título 'Don't take it from us' alinhados à esquerda em vez de centralizados; [médio] Texto do rodapé (descrição, ícones sociais, endereço, unsubscribe) alinhado à esquerda em vez de centralizado; [baixo] Parágrafo de introdução em fonte monoespaçada e título em serifa, quebrando a hierarquia tipográfica; [baixo] Card do produto deslocado e com margens diferentes; altura total ~5% maior |
| `re-05-studio-promo` | 6 | 5 | **7** | [médio] Gradiente de fundo do hero (cinza para branco) termina com corte seco logo abaixo do anel, com uma borda/linha fina visível, em vez de transição suave; [médio] Rodapé (descrição, ícones sociais, endereço, unsubscribe) alinhado à esquerda em vez de centralizado; [baixo] Texto da barra superior renderizado em fonte monoespaçada |
| `re-airbnb-review` | 8 | 7 | **7** | [médio] Avatar renderizado quadrado em vez de circular; [baixo] Link 'Report unsafe behavior' sem sublinhado |
| `re-amazon-review` | 6 | 6 | **7** | [médio] Coluna de texto ao lado da capa do livro alinhada ao topo em vez de centralizada verticalmente; [baixo] Link 'View more' e links do rodapé ('Community Guidelines', 'Amazon.com', e-mail) sem sublinhado |
| `re-apple-receipt` | 5 | 6 | **6** | [médio] Linha de TOTAL quebrada: rótulo deslocado à esquerda e borda vertical se estende para baixo da tabela; [médio] Logo 'Card' alinhado à esquerda em vez de centralizado acima do título; [baixo] Espaçamentos verticais maiores (altura ~8% maior) |
| `re-aws-verify-email` | 9 | 6 | **8** | [médio] Logo AWS alinhado à esquerda em vez de centralizado no cabeçalho |
| `re-codepen-challengers` | 7 | 8 | **8** | [baixo] Links do rodapé ('email preferences', 'instantly opt out', 'support') sem sublinhado; [baixo] Textos quebram um pouco diferente, cards levemente mais altos |
| `re-dropbox-reset-password` | 9 | 6 | **8** | [baixo] Texto em cinza mais claro que no Chromium; [baixo] Link 'more security tips' sem sublinhado |
| `re-github-access-token` | 9 | 5 | **9** | — |
| `re-google-play-policy-update` | 7 | 6 | **9** | [baixo] Pequenas diferenças de espaçamento vertical entre parágrafos |
| `re-koala-welcome` | 9 | 6 | **9** | — |
| `re-linear-login-code` | 9 | 6 | **9** | — |
| `re-netlify-welcome` | 9 | 6 | **8** | [médio] Rodapé ('Unsubscribe', 'Manage Preferences') alinhado à esquerda em vez de centralizado; [baixo] Links 'Visit the forums', 'Read the docs', 'Contact an expert' sem sublinhado |
| `re-nike-receipt` | 7 | 6 | **6** | [médio] Botão 'Track Package' desalinhado: colado à esquerda junto ao número de rastreio em vez de alinhado à direita; [médio] Links 'Men Women Kids' amontoados sem espaçamento, 'Customize' separado; [médio] Telefone e horário de atendimento juntos na mesma célula sem separação; [baixo] Botão 'Order Status' alinhado à esquerda em vez de centralizado |
| `re-notion-magic-link` | 9 | 7 | **9** | [baixo] Links 'Click here to log in…' e 'Notion.so' sem sublinhado |
| `re-papermark-year-in-review` | 9 | 5 | **5** | [alto] max-width do container ignorado: cartões ocupam os 800px inteiros; [médio] Gradientes de fundo dos cartões perdidos ou muito atenuados (cinza e verde ficam chapados); [baixo] Pílulas de países alinhadas à esquerda em vez de centralizadas |
| `re-plaid-verify-identity` | 9 | 5 | **8** | [baixo] Código '144833' sem o espaçamento entre dígitos (letter-spacing) do Chromium; [baixo] 'login@plaid.com' sem sublinhado de link |
| `re-raycast-magic-link` | 7 | 6 | **9** | [baixo] Logo do rodapé sai colorido em vez de acinzentado |
| `re-slack-confirm` | 9 | 6 | **8** | [baixo] Links do rodapé ('Our blog', 'Policies'…) sem sublinhado; [baixo] Altura ~5% maior: linha 'If you didn't request this email…' quebra em duas |
| `re-stack-overflow-tips` | 5 | 8 | **8** | [baixo] Caixa '<3' do rodapé esticada por toda a largura; [baixo] Links do rodapé sem sublinhado; [baixo] Itens da lista com espaçamento vertical maior |
| `re-stripe-welcome` | 8 | 5 | **9** | — |
| `re-twitch-reset-password` | 7 | 5 | **7** | [médio] Ícones sociais do rodapé separados (Twitter à esquerda, Facebook ao centro) em vez de agrupados e centralizados; [baixo] Links do corpo ('reset your account password', 'click here', 'Twitch Support') sem sublinhado |
| `re-vercel-invite-user` | 8 | 5 | **7** | [médio] Avatares alinhados à esquerda em vez de centralizados e sem border-radius (quadrados); seta desalinhada no topo; [baixo] Texto do botão 'Join the team' em fonte serifada |
| `re-yelp-recent-login` | 7 | 6 | **9** | — |

## Adendo 2: Blitz com 2 correções no motor

Patch: `patches/blitz-anonymous-cell-table-align.patch` (+79/−16 linhas de Rust em 2 arquivos; os 471 testes do `blitz-dom`/`blitz-tests` continuam passando, WPT não foi rodado):
1. `layout/table.rs`: filho não-célula de uma linha (`<td style="display:block">`, `div` etc.) é tratado como envolto numa célula anônima (CSS 2.2 §17.2.1) em vez de ser descartado; o bloco preenche a célula como um bloco normal, respeitando `max-width` e margens auto.
2. `stylo.rs`: `<table align=center|left|right>` vira margens auto / `float` (regra do HTML) em vez de `text-align`; `align` passa a ser case-insensitive e aceita `middle`.

Os 11 e-mails cuja renderização mudou foram re-revisados; nenhuma regressão.

| | Blitz original | Blitz corrigido | litehtml original | litehtml corrigido |
|---|---|---|---|---|
| Nota média | 7.9 | 8.2 | 6.6 | 8.1 |
| Mediana | 8 | 8 | 6 | 8 |
| Nota ≥ 8 | 31/47 | 36/47 | 15/47 | 35/47 |
| Nota ≥ 6 | 44/47 | 46/47 | 34/47 | 46/47 |
| Nota ≤ 4 | 1/47 | 0/47 | 2/47 | 0/47 |

| E-mail | Blitz antes | Blitz depois | Problemas restantes (depois) |
|---|---|---|---|
| `mailgun-billing` | 0 | **8** | [baixo] Linha 'Total' com borda superior/inferior interrompida entre as células 'Total' e '$ 33.98' (gap visível entre as células); [baixo] Espaçamentos verticais um pouco maiores (altura ~3% maior), linhas de serviço mais espaçadas |
| `re-05-studio-abandoned-cart` | 6 | **8** | [baixo] Espaçamentos verticais maiores (altura ~7% maior), especialmente acima de 'Don't take it from us' e no rodapé |
| `re-05-studio-promo` | 6 | **7** | [médio] Hero com gradiente dividido em dois blocos com borda/sombra visíveis (imagem e CTA), em vez de um fundo contínuo sem borda; [baixo] Faixa escura do topo ligeiramente mais larga que o container; [baixo] Espaçamentos verticais um pouco maiores (altura ~4% maior) |
| `re-apple-receipt` | 5 | **6** | [médio] Bloco promocional Apple Card com espaçamentos verticais excessivos (grande vão acima do logo 'Card', entre título e link e abaixo), contribuindo para altura ~16% maior; [médio] Tabela de dados (Apple ID/Billed to) com células centralizadas verticalmente e padding excessivo, desalinhando as colunas; [médio] Linha do TOTAL muito alta (~2x) com divisor vertical alongado; [baixo] Separador entre 'Write a Review' e 'Report a Problem' sem espaçamento |
| `re-dropbox-reset-password` | 9 | **9** | [baixo] Card levemente mais alto, com espaçamentos verticais um pouco maiores |
| `re-google-play-policy-update` | 7 | **8** | [baixo] Parágrafos encostam na borda direita do cartão (padding direito parcialmente ignorado); [baixo] Espaçamentos verticais um pouco maiores (altura ~4% maior) |
| `re-netlify-welcome` | 9 | **9** | [baixo] Espaçamentos verticais um pouco maiores (altura ~6% maior) |
| `re-nike-receipt` | 7 | **8** | [médio] Altura total ~7% maior por espaçamentos verticais extras entre seções (produto, Top Picks, Get Help) |
| `re-papermark-year-in-review` | 9 | **9** | [baixo] Espaçamento vertical ligeiramente maior nos cartões |
| `re-stack-overflow-tips` | 5 | **5** | [alto] Botão 'I need a break' perdeu padding/fundo: aparece como texto inline com realce azul, não como botão; [médio] Coluna de texto do hero mais estreita: título e subtítulo quebram em mais linhas e o hero fica mais alto; [baixo] Caixa '<3' do rodapé esticada por toda a largura; [baixo] Borda clara visível ao redor da área do logo no topo |
| `re-yelp-recent-login` | 7 | **8** | [baixo] Card ~4% mais largo que no Chromium e levemente deslocado para a direita; [baixo] Título 'We noticed a recent login…' e espaçamentos maiores, alongando o card (~7% mais alto) |

## Adendo 3: plataforma nova (`crates/html_view`, GPUI nativo) em loop até ≥ 9,5

Capturas da janela GPUI real (`viewer --capture`, efeitos do Hyprland desligados por regra temporária), comparadas com o Chromium; mesma rubrica, notas com uma casa decimal. Rodada 1 = plataforma nova sem correções de layout; final = após as correções (ver `.dox/vendor/AGENTS.md` e `TODO.md`).

| | Rodada 1 | Final |
|---|---|---|
| Nota média | 8.16 | **9.67** |
| Nota ≥ 9,5 | 9/47 | 41/47 |
| SSIM médio | 0.739 | 0.975 |

| E-mail | Rodada 1 | Final | Problemas restantes |
|---|---|---|---|
| `cerberus-hybrid` | 7.0 | **8.0** | [médio]{layout} Segundo bloco 'Class aptent taciti sociosqu' (seg. 1) continua sem inverter a ordem das colunas: imagem 200x200 à esquerda e texto/botão à direita, enquanto no Chromium o texto fica à esquerda e a imagem à direita; [baixo]{layout} Rodapé (Company Name/endereço/unsubscribe) e faixa verde ~3-5px mais baixos que no Chromium (altura total 2605 vs 2600px) |
| `cerberus-responsive` | 9 | **9.7** | [baixo]{paint} Marcadores das listas (bullets) do bloco 'Praesent laoreet' ~1px menores/mais finos que no Chromium; [baixo]{layout} Rodapé escuro ('View as a Web Page' / endereço / 'unsubscribe') ~2-3px mais baixo; faixa verde final deslocada ~3px (altura total +5px) |
| `client-apple-mail` | 9.5 | **9.9** | — |
| `client-gmail-reply` | 8 | **9.3** | [médio]{layout} Espaço entre a linha pequena da assinatura ('+55 11 98765-4321') e a atribuição 'Em qui., 30 de out...' ainda ~5-7px maior que no Chromium; todo o blockquote citado fica deslocado para baixo na mesma medida |
| `client-outlook-desktop` | 9.5 | **8.4** | [médio]{layout} Linhas da tabela de indicadores ~20px de altura vs ~17px no Chromium (tabela ~61px vs ~52px); [médio]{layout} Espaçamento vertical geral maior: margens entre parágrafos, lista e assinatura mais altas; último parágrafo 'Ricardo, pode consolidar...' em y≈418 vs y≈372 no Chromium (+46px, ~12%); [baixo]{layout} Linhas do cabeçalho de resposta (De/Enviada em/Para/Assunto) com espaçamento ~20px vs ~18px |
| `leemunroe-responsive` | 8 | **9.9** | [baixo]{layout} Nenhum defeito visível: fundo cinza (#f4f5f6) agora propagado ao canvas até o fim da viewport |
| `mailgun-billing` | 8 | **9.8** | [baixo]{layout} Nenhum defeito visível: rótulo 'Total' agora na posição correta (célula width=80%), altura idêntica (776px) |
| `mjml-austin` | 8 | **9.8** | [baixo]{paint} Nenhuma diferença visível além de antialiasing de fonte; altura idêntica (2619px), hero com padrão de fundo, botões, imagens em duas colunas, mapa e rodapé corretos |
| `mjml-black-friday` | 9.5 | **9.8** | [baixo]{layout} Altura total ~1% maior (960 vs 950px), diferença de métricas de linha distribuída entre os blocos de texto |
| `mjml-newsletter` | 9.5 | **9.6** | [baixo]{paint} Título 'Here is what you've missed' (font-family:Helvetica Neue sem genérico) cai em fonte sem serifa, enquanto Chromium usa a serifa padrão; altura/posição ~2px diferente; [baixo]{layout} Rodapé 'Stay in touch!' e ícones sociais ~1px acima do Chromium |
| `mjml-racoon` | 8.5 | **9.7** | [baixo]{layout} Seções a partir de 'ANDERSON SWEATER'/'SUMMER COLLECTION' deslocadas ~1px para baixo (altura 2558 vs 2557px); o título 'PAYMENT METHODS' agora cabe em uma linha como no Chromium (corrigido desde a it9) |
| `mjml-real-estate` | 9.0 | **9.7** | [baixo]{layout} Altura total levemente maior (~1-2%), conteúdo inferior desloca poucos px |
| `mjml-recast` | 9 | **9.8** | [baixo]{layout} Deriva vertical acumulada de ~1-2px a partir da lista de artigos (títulos/imagens levemente mais baixos que no Chromium) |
| `mjml-ticketshop` | 9.5 | **9.8** | — |
| `mjml-welcome-email` | 9 | **9.9** | [baixo]{layout} 'Thanks, / The [[CompanyName]] Team' ~1px mais alto que no Chromium |
| `postmark-comment-notification` | 7 | **9.9** | [baixo]{layout} Nenhum defeito visível: fundo cinza do body agora preenche toda a viewport como no Chromium |
| `postmark-invoice` | 8.5 | **9.9** | — |
| `postmark-receipt` | 7.5 | **9.8** | — |
| `postmark-welcome` | 8.5 | **9.8** | — |
| `re-02-matte-welcome` | 7 | **9.8** | [baixo]{paint} Nenhuma diferença perceptível além de fonte; altura idêntica (2427px), sombra do cartão, imagens rotacionadas sobrepostas ao título 'Try Composer today' e lista com setas iguais ao Chromium |
| `re-03-protocol-product-update` | 7.5 | **9.8** | [baixo]{layout} Altura levemente diferente por métricas de linha; layout, imagens, badge 'Feature' e cards do blog idênticos |
| `re-04-arcane-newsletter` | 7.5 | **9.8** | — |
| `re-04-arcane-order-confirmation` | 7 | **9.9** | — |
| `re-05-studio-abandoned-cart` | 8.5 | **9.7** | [baixo]{paint} Estrelas '★★★★★' (20px) do depoimento renderizadas bem maiores (~2x) que no Chromium, por glifo de fallback diferente; posição e alinhamento corretos |
| `re-05-studio-promo` | 7.5 | **9.6** | [baixo]{paint} Estrelas '★★★★★' do depoimento continuam maiores/mais escuras que no Chromium (glifo de fallback com métrica diferente) |
| `re-airbnb-review` | 7 | **9.7** | [baixo]{layout} Rodapé (endereço/'Report unsafe behavior') ~2-3px mais baixo que no Chromium (altura total 992 vs 989px) |
| `re-amazon-review` | 6.5 | **9.8** | [baixo]{layout} Espaçamentos verticais do rodapé com diferença de ~1px; borda superior laranja e layout de duas colunas idênticos ao Chromium |
| `re-apple-receipt` | 6 | **9.0** | [médio]{layout} Bloco Apple Card: espaço entre o logo 'Card' e o título 'Save 3% on all your Apple purchases.' ~14px maior que no Chromium (margens pai/primeiro filho não colapsam); todo o restante do e-mail desce ~14px (1412 vs 1398px) |
| `re-aws-verify-email` | 8.5 | **9.8** | — |
| `re-codepen-challengers` | 6 | **9.9** | — |
| `re-dropbox-reset-password` | 8.5 | **9.9** | [baixo]{layout} Nenhum defeito visível: fundo azul-claro do body agora preenche toda a viewport abaixo do cartão |
| `re-github-access-token` | 9.5 | **9.6** | [baixo]{paint} Separador '・' (U+30FB) no rodapé ainda com avanço estreito: 'GitHub, Inc. ・88 Colin...' sem o respiro de largura total do Chromium, linha ~8px mais estreita; idem entre 'Your security audit log' e 'Contact support' |
| `re-google-play-policy-update` | 8.5 | **9.8** | [baixo]{layout} Diferenças sub-pixel de espaçamento vertical; faixas diagonais azuis, cartão e rodapé 'Connect with us' corretos |
| `re-koala-welcome` | 9.5 | **9.9** | — |
| `re-linear-login-code` | 9.5 | **9.8** | — |
| `re-netlify-welcome` | 8.5 | **9.8** | [baixo]{paint} Marcadores (bullets) da lista 'Deploy your first project…' visivelmente menores que os do Chromium |
| `re-nike-receipt` | 8.5 | **9.6** | [baixo]{layout} Seção 'Get Help': linhas de links ~3px mais espaçadas que no Chromium, empurrando telefone, 'Nike.com' e rodapé ~4-6px para baixo (altura 2175 vs 2169px); [baixo]{paint} Ícone do telefone com traço/proporção levemente diferente |
| `re-notion-magic-link` | 9.5 | **9.8** | [baixo]{paint} Nenhuma diferença perceptível além de fonte; bloco de código, logo e rodapé alinhados como no Chromium |
| `re-papermark-year-in-review` | 7.5 | **9.8** | [baixo]{paint} Nenhum defeito visível: cards com gradiente agora com os 4 cantos arredondados e altura total idêntica (2235px); só diferenças de AA de fonte |
| `re-plaid-verify-identity` | 8.0 | **9.8** | [baixo]{layout} Altura ~1.7% maior (610 vs 600px), espaçamento vertical levemente maior no rodapé 'SECURELY POWERED BY PLAID' |
| `re-raycast-magic-link` | 8 | **9.2** | [médio]{paint} Emojis 👉/👈 do link 'Click here to sign in' continuam desenhados como glifos monocromáticos de contorno na cor do link, em vez dos emojis coloridos do Chromium; o 🪄 do título sai colorido corretamente |
| `re-slack-confirm` | 8.5 | **9.8** | — |
| `re-stack-overflow-tips` | 6 | **9.2** | [médio]{layout} Selo '<3' no rodapé continua ocupando toda a largura do conteúdo (~620px) com borda cinza esticada, em vez de caixa compacta (~26px) ajustada ao texto como no Chromium; [baixo]{paint} Marcadores (bullets) da lista de dicas de busca menores que no Chromium |
| `re-stripe-welcome` | 8.5 | **9.8** | [baixo]{paint} Nenhuma diferença perceptível além de fonte; altura idêntica (819px), botão, divisores e links iguais ao Chromium |
| `re-twitch-reset-password` | 8.5 | **9.9** | — |
| `re-vercel-invite-user` | 7.5 | **9.8** | — |
| `re-yelp-recent-login` | 7.5 | **9.8** | — |

## Adendo 4: segunda rodada de correções

Política: e-mail sem doctype é renderizado em modo padrão (é como aparece dentro do Gmail/Outlook web); as referências do Chromium desses e-mails foram regeneradas com `<!DOCTYPE html>`, a mesma entrada para os dois motores.

Nota média final: **9.77** · ≥ 9,5: 46/47 · mínimo 9.2 · SSIM médio 0.985.

| E-mail | Nota | Problemas restantes |
|---|---|---|
| `cerberus-hybrid` | 9.5 | [baixo]{layout} Rodapé (View as a Web Page / Company Name / unsubscribe) e faixa verde final ~3-5px mais baixos que no Chromium (altura total 2605 vs 2600px) |
| `cerberus-responsive` | 9.7 | [baixo]{paint} Marcadores das listas (bullets) do bloco 'Praesent laoreet' ~1px menores/mais finos que no Chromium; [baixo]{layout} Rodapé escuro ('View as a Web Page' / endereço / 'unsubscribe') ~2-3px mais baixo; faixa verde final deslocada ~3px (altura total +5px) |
| `client-apple-mail` | 9.9 | — |
| `client-gmail-reply` | 9.9 | — |
| `client-outlook-desktop` | 9.6 | [baixo]{layout} Linhas da tabela de indicadores ~1-2px mais altas (tabela termina em y≈141 vs y≈137); conteúdo abaixo (lista, assinatura, cabeçalho de resposta, último parágrafo) deslocado ~4px para baixo |
| `leemunroe-responsive` | 9.9 | [baixo]{layout} Nenhum defeito visível: fundo cinza (#f4f5f6) agora propagado ao canvas até o fim da viewport |
| `mailgun-billing` | 9.8 | [baixo]{layout} Nenhum defeito visível: rótulo 'Total' agora na posição correta (célula width=80%), altura idêntica (776px) |
| `mjml-austin` | 9.8 | [baixo]{paint} Nenhuma diferença visível além de antialiasing de fonte; altura idêntica (2619px), hero com padrão de fundo, botões, imagens em duas colunas, mapa e rodapé corretos |
| `mjml-black-friday` | 9.8 | [baixo]{layout} Altura total ~1% maior (960 vs 950px), diferença de métricas de linha distribuída entre os blocos de texto |
| `mjml-newsletter` | 9.6 | [baixo]{paint} Título 'Here is what you've missed' (font-family:Helvetica Neue sem genérico) cai em fonte sem serifa, enquanto Chromium usa a serifa padrão; altura/posição ~2px diferente; [baixo]{layout} Rodapé 'Stay in touch!' e ícones sociais ~1px acima do Chromium |
| `mjml-racoon` | 9.7 | [baixo]{layout} Seções a partir de 'ANDERSON SWEATER'/'SUMMER COLLECTION' deslocadas ~1px para baixo (altura 2558 vs 2557px); o título 'PAYMENT METHODS' agora cabe em uma linha como no Chromium (corrigido desde a it9) |
| `mjml-real-estate` | 9.7 | [baixo]{layout} Altura total levemente maior (~1-2%), conteúdo inferior desloca poucos px |
| `mjml-recast` | 9.8 | [baixo]{layout} Deriva vertical acumulada de ~1-2px a partir da lista de artigos (títulos/imagens levemente mais baixos que no Chromium) |
| `mjml-ticketshop` | 9.8 | — |
| `mjml-welcome-email` | 9.9 | [baixo]{layout} 'Thanks, / The [[CompanyName]] Team' ~1px mais alto que no Chromium |
| `postmark-comment-notification` | 9.9 | [baixo]{layout} Nenhum defeito visível: fundo cinza do body agora preenche toda a viewport como no Chromium |
| `postmark-invoice` | 9.9 | — |
| `postmark-receipt` | 9.8 | — |
| `postmark-welcome` | 9.8 | — |
| `re-02-matte-welcome` | 9.8 | — |
| `re-03-protocol-product-update` | 9.8 | [baixo]{layout} Altura levemente diferente por métricas de linha; layout, imagens, badge 'Feature' e cards do blog idênticos |
| `re-04-arcane-newsletter` | 9.8 | — |
| `re-04-arcane-order-confirmation` | 9.9 | — |
| `re-05-studio-abandoned-cart` | 9.7 | [baixo]{paint} Estrelas '★★★★★' do depoimento continuam maiores/mais escuras que no Chromium (glifo de fallback diferente); posição e alinhamento corretos |
| `re-05-studio-promo` | 9.6 | [baixo]{paint} Estrelas '★★★★★' do depoimento continuam maiores/mais escuras que no Chromium (glifo de fallback com métrica diferente) |
| `re-airbnb-review` | 9.7 | [baixo]{layout} Rodapé (endereço/'Report unsafe behavior') ~2-3px mais baixo que no Chromium (altura total 992 vs 989px) |
| `re-amazon-review` | 9.8 | [baixo]{layout} Espaçamentos verticais do rodapé com diferença de ~1px; borda superior laranja e layout de duas colunas idênticos ao Chromium |
| `re-apple-receipt` | 9.7 | [baixo]{layout} Pequenos desvios verticais de ~1-2px no bloco Apple Card e rodapé (altura total 1397 vs 1398px); não perceptível |
| `re-aws-verify-email` | 9.8 | — |
| `re-codepen-challengers` | 9.9 | — |
| `re-dropbox-reset-password` | 9.9 | [baixo]{layout} Nenhum defeito visível: fundo azul-claro do body agora preenche toda a viewport abaixo do cartão |
| `re-github-access-token` | 9.6 | [baixo]{paint} Separador '・' (U+30FB) no rodapé ainda com avanço estreito: 'GitHub, Inc. ・88 Colin...' sem o respiro de largura total do Chromium, linha ~8px mais estreita; idem entre 'Your security audit log' e 'Contact support' |
| `re-google-play-policy-update` | 9.8 | [baixo]{layout} Diferenças sub-pixel de espaçamento vertical; faixas diagonais azuis, cartão e rodapé 'Connect with us' corretos |
| `re-koala-welcome` | 9.9 | — |
| `re-linear-login-code` | 9.8 | — |
| `re-netlify-welcome` | 9.9 | — |
| `re-nike-receipt` | 9.6 | [baixo]{layout} Seção 'Get Help': linhas de links ~3px mais espaçadas que no Chromium, empurrando telefone, 'Nike.com' e rodapé ~4-6px para baixo (altura 2175 vs 2169px); [baixo]{paint} Ícone do telefone com traço/proporção levemente diferente |
| `re-notion-magic-link` | 9.8 | [baixo]{paint} Nenhuma diferença perceptível além de fonte; bloco de código, logo e rodapé alinhados como no Chromium |
| `re-papermark-year-in-review` | 9.8 | [baixo]{paint} Nenhum defeito visível: cards com gradiente agora com os 4 cantos arredondados e altura total idêntica (2235px); só diferenças de AA de fonte |
| `re-plaid-verify-identity` | 9.8 | [baixo]{layout} Altura ~1.7% maior (610 vs 600px), espaçamento vertical levemente maior no rodapé 'SECURELY POWERED BY PLAID' |
| `re-raycast-magic-link` | 9.2 | [médio]{paint} Emojis 👉/👈 do link 'Click here to sign in' continuam desenhados como glifos monocromáticos de contorno na cor do link, em vez dos emojis coloridos do Chromium; o 🪄 do título sai colorido corretamente |
| `re-slack-confirm` | 9.8 | — |
| `re-stack-overflow-tips` | 9.8 | — |
| `re-stripe-welcome` | 9.8 | [baixo]{paint} Nenhuma diferença perceptível além de fonte; altura idêntica (819px), botão, divisores e links iguais ao Chromium |
| `re-twitch-reset-password` | 9.9 | — |
| `re-vercel-invite-user` | 9.8 | — |
| `re-yelp-recent-login` | 9.8 | — |

## Adendo 5: várias larguras (375, 600, 800, 1280 px)

Cada e-mail foi renderizado no Chromium e no html_view em 4 larguras de viewport. Nos e-mails mais largos que o viewport, a referência é recortada no viewport (o que o leitor vê antes de rolar horizontalmente). Foram revisados visualmente os pares com SSIM < 0,985 (60 combinações); os demais ficaram praticamente idênticos ao Chromium.

| Largura | SSIM médio | Revisados | Nota média (revisados) | < 9,5 |
|---|---|---|---|---|
| 375 | 0.969 | 32 | 9.76 | `mailgun-billing` 9.0, `re-plaid-verify-identity` 9.4 |
| 600 | 0.982 | 14 | 9.71 | `mailgun-billing` 9.0 |
| 800 | 0.989 | 9 | 9.78 | — |
| 1280 | 0.994 | 5 | 9.72 | — |

Total revisado: média **9.75**, 57/60 ≥ 9,5. Os 3 abaixo vêm todos do mesmo caso: `font-weight:800` com Arial pega o arquivo "Arial Black" (fontes do Windows instaladas no Linux); o Chromium usa o Bold.

Correções desta rodada: política de fontes do navegador; células `display:block` empilhadas numa única célula anônima; rolagem horizontal; bordas colapsadas contadas uma vez; imagens reduzidas com filtro; cantos do clip arredondado em preenchimentos parciais; altura definida de imagens sob `max-width`; baseline de inline-blocks (botões, tabelas); colunas percentuais somando 100%; marcadores alinhados à primeira linha de um item de bloco.

| Item | Nota | Problemas |
|---|---|---|
| `cerberus-hybrid@1280` | 9.8 | [baixo] Rodapé e faixa verde ~3-5px mais baixos; layout de colunas idêntico |
| `cerberus-hybrid@375` | 9.7 | [baixo] Texto do rodapé escuro (Company Name / endereço / telefone) com entrelinha levemente maior, empurrando 'unsubscribe' e a faixa verde ~5-7px para baixo |
| `cerberus-hybrid@600` | 9.6 | [baixo] Rodapé escuro ~4px mais alto (espaçamento das linhas com <br><br> e line-height:15px maior que no Chromium); restante idêntico, incluindo colunas híbridas 2+1 |
| `cerberus-hybrid@800` | 9.7 | [baixo] Rodapé escuro ~3px mais baixo (altura total +5px); grade 2 e 3 colunas, blocos texto/imagem invertidos e faixa verde idênticos |
| `cerberus-responsive@1280` | 9.7 | [baixo] Rodapé/faixa verde ~3–5px mais baixos (altura total +5px); demais blocos idênticos |
| `cerberus-responsive@375` | 9.5 | [baixo] Rodapé escuro ~5px mais alto (linhas com <br><br> e line-height:15px) |
| `cerberus-responsive@600` | 9.6 | [baixo] Rodapé escuro ~4px mais alto (espaçamento extra nas quebras <br><br> com line-height:15px) |
| `cerberus-responsive@800` | 9.8 | [baixo] Rodapé (endereço/unsubscribe) e faixa verde ~2-4px mais baixos; restante idêntico |
| `client-apple-mail@375` | 9.9 | [baixo] Diferenças apenas de antialiasing; layout, imagem e blockquote idênticos |
| `client-gmail-reply@375` | 9.9 | — |
| `client-gmail-reply@600` | 9.9 | [baixo] Sem diferenças visíveis; blockquote com borda, lista e destaque idênticos |
| `client-outlook-desktop@375` | 9.9 | — |
| `leemunroe-responsive@375` | 9.8 | [baixo] Borda esquerda do cartão branco ~1px mais à direita (x=5 vs 4) e botão 'Call To Action' ~1-2px mais estreito; imperceptível |
| `leemunroe-responsive@600` | 9.9 | — |
| `mailgun-billing@375` | 9.0 | [médio] Títulos '$33.98 Paid' (h1) e 'Thanks for using Acme Inc.' (h2) renderizados num peso bem mais pesado e largo que no Chromium (~15% mais largos, aparência 'black'); Chromium usa o Bold normal da fonte; [baixo] Restante (tabela de itens, totais, rodapé) idêntico |
| `mailgun-billing@600` | 9.0 | [médio] h1 '$33.98 Paid' e h2 'Thanks for using Acme Inc.' visivelmente mais pesados/largos que no Chromium (peso 800 não mapeado para Bold) |
| `mjml-austin@375` | 9.8 | [baixo] Botões 'RSVP Today'/'Book an Appointment' ~1-2px mais baixos; sombra do card e rodapé equivalentes |
| `mjml-black-friday@375` | 9.9 | — |
| `mjml-newsletter@800` | 9.9 | [baixo] Sem diferenças visíveis; sombra da imagem de código e colunas ícone+texto idênticas |
| `mjml-racoon@375` | 9.7 | [baixo] Deriva vertical cumulativa de ~5-7px ao longo do e-mail (títulos/preços dos produtos e rodapé escuro descem alguns px em relação ao Chromium); [baixo] Seção 'Discover our Summer Collection' e rodapé laranja ~5px mais baixos que no Chromium |
| `mjml-real-estate@1280` | 9.8 | — |
| `mjml-real-estate@375` | 9.8 | [baixo] Deriva vertical acumulada de ~3px a partir da seção 'Lorem Ipsum' (métricas de linha); layout, cards sobre imagem de fundo, setas e botões idênticos ao Chromium |
| `mjml-real-estate@600` | 9.8 | [baixo] Deriva vertical de 1-2px na segunda metade (seções 'Nullam dictum', '2. offer', rodapé) |
| `mjml-real-estate@800` | 9.8 | — |
| `mjml-recast@1280` | 9.8 | [baixo] Sem diferenças visíveis além de antialiasing de fonte |
| `mjml-recast@375` | 9.8 | [baixo] Deriva de 1–2px; colunas empilham corretamente no mobile, miniaturas centralizadas como no Chromium |
| `mjml-recast@600` | 9.8 | [baixo] Deriva de ~1px por bloco; colunas texto+miniatura, divisores e ícones sociais idênticos |
| `mjml-recast@800` | 9.8 | [baixo] Deriva de ~1px; sem diferenças de layout |
| `mjml-welcome-email@375` | 9.9 | — |
| `postmark-receipt@375` | 9.9 | — |
| `postmark-welcome@375` | 9.9 | [baixo] Sem diferenças visíveis; botão com sombra e caixas de informação idênticos |
| `re-02-matte-welcome@375` | 9.7 | [baixo] Imagens hero e decorativas (colagem verde em meio-tom) reamostradas com filtro mais suave: o padrão de pontos/moiré visível no Chromium aparece mais liso no html_view; layout, cartão, sombra e textos idênticos |
| `re-02-matte-welcome@600` | 9.8 | [baixo] Mesma diferença de reamostragem nas imagens em meio-tom; layout, quebras de linha, divisores e rodapé idênticos |
| `re-02-matte-welcome@800` | 9.8 | [baixo] Mesma diferença de reamostragem nas imagens em meio-tom; cartão de 400px centralizado, sombra e espaçamentos idênticos |
| `re-03-protocol-product-update@375` | 9.9 | — |
| `re-04-arcane-newsletter@375` | 9.8 | [baixo] Deriva vertical de 1-3px em blocos de texto; layout, quebras de linha e imagens idênticos ao Chromium |
| `re-04-arcane-newsletter@600` | 9.8 | [baixo] Sem diferenças visíveis além de antialiasing; numeração em caixas, citação e rodapé idênticos |
| `re-04-arcane-order-confirmation@375` | 9.9 | [baixo] Deriva de 1px em títulos serifados; tabela de totais idêntica |
| `re-05-studio-promo@375` | 9.8 | [baixo] Lista 'Try your ring risk-free…' e card de depoimento com deriva de ~1-2px; sem diferenças visíveis |
| `re-airbnb-review@375` | 9.8 | [baixo] Bloco final (linha divisória, endereço e 'Report unsafe behavior') ~3px mais baixo |
| `re-apple-receipt@375` | 9.7 | [baixo] Blocos de texto (bloco de produto e rodapé) deslocados 1–3px verticalmente em relação ao Chromium; layout, cortes à direita e alinhamentos idênticos |
| `re-apple-receipt@600` | 9.8 | [baixo] Diferenças de 1–2px em posições verticais; tabelas de cabeçalho, linha TOTAL e rodapé idênticos, corte à direita igual ao Chromium |
| `re-apple-receipt@800` | 9.8 | [baixo] Conteúdo ~1px acima a partir do bloco de itens; sem diferenças visíveis de layout |
| `re-aws-verify-email@375` | 9.9 | [baixo] Sem diferenças visíveis além de antialiasing |
| `re-aws-verify-email@800` | 9.9 | — |
| `re-google-play-policy-update@375` | 9.8 | — |
| `re-google-play-policy-update@600` | 9.8 | — |
| `re-nike-receipt@1280` | 9.5 | [baixo] Linhas de 'Get Help' ~3px mais altas; rodapé deslocado ~6px |
| `re-nike-receipt@375` | 9.5 | [baixo] Grade 'Get Help' (Shipping Status / How to Return…): cada linha ~3px mais alta, empurrando o restante do rodapé ~6px para baixo; demais seções (overflow à direita igual nos dois) idênticas |
| `re-nike-receipt@600` | 9.5 | [baixo] Linhas de links em 'Get Help' ~3px mais altas cada; rodapé (telefone, Nike.com, links legais) deslocado ~6px para baixo |
| `re-nike-receipt@800` | 9.5 | [baixo] Linhas de 'Get Help' ~3px mais altas; rodapé deslocado ~6px |
| `re-notion-magic-link@375` | 9.9 | [baixo] Sem diferenças visíveis além de antialiasing |
| `re-papermark-year-in-review@375` | 9.6 | [baixo] Colunas da tabela 'Your activity' (25/50/500) deslocadas ~2–3px em relação ao Chromium; [baixo] Gradientes radiais, raios e pílulas de países idênticos |
| `re-papermark-year-in-review@600` | 9.9 | — |
| `re-plaid-verify-identity@375` | 9.4 | [baixo] Rodapé 'SECURELY POWERED BY PLAID.' em peso bem mais pesado e ~10% mais largo que no Chromium (aparência extra-black); [baixo] Resto (escudo, código 144833, textos) idêntico |
| `re-slack-confirm@375` | 9.7 | [baixo] Sublinhado do link 'Slack' estende-se sobre o espaço em branco no fim da linha (Chromium não sublinha o espaço pendente antes da quebra); [baixo] Parágrafo 'If you didn't request…' com entrelinha ~1px maior |
| `re-stack-overflow-tips@375` | 9.8 | — |
| `re-stack-overflow-tips@600` | 9.8 | — |
| `re-twitch-reset-password@375` | 9.9 | [baixo] Sem diferenças visíveis além de antialiasing |
| `re-vercel-invite-user@375` | 9.9 | [baixo] Diferenças apenas de antialiasing/1px; layout idêntico |
