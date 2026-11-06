//! Brand logos (BIMI) already checked, per sender domain, so each domain costs the
//! network once in a while rather than on every message.

use rusqlite::{params, OptionalExtension};

use crate::database::Database;

/// What was found for a domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrandLogo {
    /// The verified SVG; `None` when the domain has none to show.
    pub svg: Option<Vec<u8>>,
    /// When it was checked (Unix seconds).
    pub checked_at: i64,
}

impl Database {
    /// What was last found for `domain` (any case), if it was ever checked.
    pub fn brand_logo(&self, domain: &str) -> rusqlite::Result<Option<BrandLogo>> {
        self.conn()
            .query_row(
                "SELECT svg, checked_at FROM brand_logos WHERE domain = ?1",
                params![domain],
                |row| {
                    Ok(BrandLogo {
                        svg: row.get(0)?,
                        checked_at: row.get(1)?,
                    })
                },
            )
            .optional()
    }

    /// Records what was found for `domain`, replacing the last finding.
    pub fn set_brand_logo(
        &self,
        domain: &str,
        svg: Option<&[u8]>,
        checked_at: i64,
    ) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO brand_logos (domain, svg, checked_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(domain) DO UPDATE SET svg = excluded.svg, checked_at = excluded.checked_at",
            params![domain, svg, checked_at],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::remote_db;

    #[test]
    fn a_finding_is_kept_per_domain_and_replaced() {
        let (_file, db, _) = remote_db();
        assert_eq!(db.brand_logo("nubank.com.br").unwrap(), None);
        db.set_brand_logo("nubank.com.br", Some(b"<svg/>"), 10)
            .unwrap();
        db.set_brand_logo("airbnb.com", None, 11).unwrap();
        let logo = db.brand_logo("NUBANK.com.br").unwrap().unwrap();
        assert_eq!(logo.svg.as_deref(), Some(&b"<svg/>"[..]));
        assert_eq!(logo.checked_at, 10);
        assert_eq!(db.brand_logo("airbnb.com").unwrap().unwrap().svg, None);
        // A later check replaces the earlier one.
        db.set_brand_logo("nubank.com.br", None, 20).unwrap();
        let logo = db.brand_logo("nubank.com.br").unwrap().unwrap();
        assert_eq!((logo.svg, logo.checked_at), (None, 20));
    }
}
