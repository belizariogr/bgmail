//! Dados de exemplo (mock) usados pelo protótipo visual.
//!
//! Nenhuma lógica real de e-mail vive aqui — apenas estruturas estáticas para
//! popular a interface enquanto validamos o layout e a performance. Quando a
//! camada de domínio for implementada, estes tipos serão substituídos pelos
//! modelos reais (provavelmente em um crate `mail_core`).

use gpui::SharedString;

/// Tipo semântico de uma caixa de e-mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxKind {
    Inbox,
    Drafts,
    Sent,
    Junk,
    Trash,
    Archive,
}

/// Uma caixa de e-mail dentro de uma conta.
#[derive(Debug, Clone)]
pub struct Mailbox {
    pub kind: MailboxKind,
    pub name: SharedString,
    pub unread: usize,
}

impl Mailbox {
    fn new(kind: MailboxKind, name: &'static str, unread: usize) -> Self {
        Self {
            kind,
            name: name.into(),
            unread,
        }
    }
}

/// Uma conta de e-mail conectada.
#[derive(Debug, Clone)]
pub struct Account {
    pub name: SharedString,
    pub email: SharedString,
    pub mailboxes: Vec<Mailbox>,
}

/// Uma mensagem de e-mail.
#[derive(Debug, Clone)]
pub struct Message {
    pub sender: SharedString,
    pub sender_email: SharedString,
    pub subject: SharedString,
    pub preview: SharedString,
    pub body: SharedString,
    pub time: SharedString,
    pub unread: bool,
    pub starred: bool,
    pub has_attachment: bool,
}

/// Caixas padrão presentes em qualquer conta.
fn default_mailboxes(inbox_unread: usize) -> Vec<Mailbox> {
    vec![
        Mailbox::new(MailboxKind::Inbox, "Caixa de entrada", inbox_unread),
        Mailbox::new(MailboxKind::Drafts, "Rascunhos", 0),
        Mailbox::new(MailboxKind::Sent, "Enviados", 0),
        Mailbox::new(MailboxKind::Junk, "Spam", 3),
        Mailbox::new(MailboxKind::Trash, "Lixeira", 0),
        Mailbox::new(MailboxKind::Archive, "Arquivo", 0),
    ]
}

/// Contas de exemplo (um Gmail e um IMAP).
pub fn sample_accounts() -> Vec<Account> {
    vec![
        Account {
            name: "Pessoal".into(),
            email: "voce@gmail.com".into(),
            mailboxes: default_mailboxes(7),
        },
        Account {
            name: "Trabalho".into(),
            email: "voce@empresa.com".into(),
            mailboxes: default_mailboxes(2),
        },
    ]
}

/// Mensagens de exemplo da caixa de entrada.
pub fn sample_messages() -> Vec<Message> {
    let raw = [
        (
            "GitHub",
            "noreply@github.com",
            "[zed-industries/zed] Nova release v0.200.0",
            "A nova versão traz melhorias de performance no GPUI e correções...",
            true,
            false,
            true,
            "09:42",
        ),
        (
            "Maria Silva",
            "maria.silva@empresa.com",
            "Reunião de planejamento — quinta-feira",
            "Oi! Podemos confirmar a reunião para quinta às 14h? Segue a pauta em anexo.",
            true,
            true,
            true,
            "09:05",
        ),
        (
            "Newsletter Rust",
            "this-week@rust-lang.org",
            "This Week in Rust #600",
            "As novidades do ecossistema Rust desta semana, incluindo async, GUIs e mais.",
            true,
            false,
            false,
            "08:30",
        ),
        (
            "Banco Digital",
            "alertas@banco.com",
            "Sua fatura está disponível",
            "A fatura do mês está fechada. Acesse o app para visualizar os detalhes.",
            false,
            false,
            false,
            "Ontem",
        ),
        (
            "João Pereira",
            "joao@startup.io",
            "Re: Proposta de parceria",
            "Perfeito, faz sentido. Vamos seguir com o contrato então. Obrigado!",
            false,
            true,
            false,
            "Ontem",
        ),
        (
            "Equipe rMail",
            "ola@rmail.app",
            "Bem-vindo ao rMail",
            "Obrigado por experimentar o rMail — um cliente de e-mail rápido e elegante.",
            false,
            false,
            false,
            "Seg",
        ),
        (
            "Conferência DevConf",
            "info@devconf.com",
            "Sua inscrição foi confirmada",
            "Nos vemos em outubro! Guarde seu ingresso e o cronograma das palestras.",
            false,
            false,
            true,
            "Seg",
        ),
        (
            "Loja Online",
            "pedidos@loja.com",
            "Seu pedido foi enviado",
            "O pedido #48213 saiu para entrega e chega em até 3 dias úteis.",
            false,
            false,
            false,
            "Dom",
        ),
    ];

    raw.into_iter()
        .map(
            |(sender, email, subject, preview, unread, starred, attach, time)| Message {
                sender: sender.into(),
                sender_email: email.into(),
                subject: subject.into(),
                preview: preview.into(),
                body: format!(
                    "{preview}\n\nEste é um corpo de mensagem de exemplo usado no mock visual do \
                     rMail. O conteúdo real será renderizado a partir de HTML/texto quando a \
                     camada de domínio for implementada.\n\nAtenciosamente,\n{sender}"
                )
                .into(),
                time: time.into(),
                unread,
                starred,
                has_attachment: attach,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_have_default_mailboxes() {
        let accounts = sample_accounts();
        assert_eq!(accounts.len(), 2);
        for account in &accounts {
            assert_eq!(account.mailboxes.len(), 6);
            assert_eq!(account.mailboxes[0].kind, MailboxKind::Inbox);
        }
    }

    #[test]
    fn sample_messages_are_populated() {
        let messages = sample_messages();
        assert!(!messages.is_empty());
        assert!(messages.iter().any(|m| m.unread));
        assert!(messages.iter().any(|m| m.has_attachment));
    }

    #[test]
    fn unread_count_matches_first_account() {
        let accounts = sample_accounts();
        assert_eq!(accounts[0].mailboxes[0].unread, 7);
    }
}
