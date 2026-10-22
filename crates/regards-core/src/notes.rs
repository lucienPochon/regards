//! Notes about people (ADR-0020): which ones an agent may not keep without
//! the person looking first. A note that carries a link, an email address
//! or payment details is how an email could plant something for later
//! ("invoices now go to this IBAN"), so such a note waits for approval.

/// Why a note waits for the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Review {
    Link,
    Address,
    BankDetails,
}

impl Review {
    /// As the person is told: "It has … in it."
    pub fn describe(self) -> &'static str {
        match self {
            Self::Link => "a link",
            Self::Address => "an email address",
            Self::BankDetails => "bank or card details",
        }
    }
}

/// Whether a note's text needs the person's approval, and why.
pub fn needs_review(text: &str) -> Option<Review> {
    if has_link(text) {
        Some(Review::Link)
    } else if has_address(text) {
        Some(Review::Address)
    } else if has_iban(text) || has_card_number(text) {
        Some(Review::BankDetails)
    } else {
        None
    }
}

fn has_link(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("://") || lower.contains("www.") || lower.contains("mailto:")
}

/// Something like `name@domain.tld`.
fn has_address(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || "<>()[]{},;:\"'".contains(c))
        .any(|word| {
            let word = word.trim_end_matches(['.', '!', '?']);
            let Some((local, domain)) = word.split_once('@') else {
                return false;
            };
            !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
        })
}

/// An IBAN, spaced in groups or not: a word that starts with two letters and
/// two digits, then at least eleven letters or digits, most of them digits.
fn has_iban(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).any(|at| {
        let starts_word = at == 0 || !chars[at - 1].is_alphanumeric();
        let head = &chars[at..];
        if !starts_word
            || head.len() < 4
            || !head[0].is_ascii_alphabetic()
            || !head[1].is_ascii_alphabetic()
            || !head[2].is_ascii_digit()
            || !head[3].is_ascii_digit()
        {
            return false;
        }
        // The rest, across single spaces between groups.
        let mut tail = Vec::new();
        let mut rest = head[4..].iter().peekable();
        while let Some(&c) = rest.next() {
            if c.is_ascii_alphanumeric() {
                tail.push(c);
            } else if c == ' ' && rest.peek().is_some_and(|next| next.is_ascii_alphanumeric()) {
                continue;
            } else {
                break;
            }
        }
        tail.len() >= 11 && tail.iter().filter(|c| c.is_ascii_digit()).count() >= 8
    })
}

/// A card number: 13 to 19 digits, perhaps in groups, that pass the Luhn
/// check (so phone numbers mostly don't).
fn has_card_number(text: &str) -> bool {
    let mut digits = Vec::new();
    let mut runs = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(digit) = c.to_digit(10) {
            digits.push(digit);
        } else if (c == ' ' || c == '-') && !digits.is_empty() && chars.peek().is_some_and(char::is_ascii_digit) {
            continue;
        } else if !digits.is_empty() {
            runs.push(std::mem::take(&mut digits));
        }
    }
    runs.push(digits);
    runs.iter().any(|run| (13..=19).contains(&run.len()) && luhn(run))
}

fn luhn(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_facts_need_no_review() {
        for text in [
            "Decides on renewals for Client SA.",
            "Prefers calls before 10:00.",
            "Mobile +41 79 555 01 23; best after lunch.",
            "Tel 0041 79 555 01 23.",
            "Invoices by the 25th, Q3 and Q4.",
            "Room AB12 on the 3rd floor.",
            "Her assistant is Julie.",
        ] {
            assert_eq!(needs_review(text), None, "{text}");
        }
    }

    #[test]
    fn links_addresses_and_payment_details_wait() {
        assert_eq!(needs_review("Portal: https://pay.example.test/x"), Some(Review::Link));
        assert_eq!(needs_review("See www.example.test"), Some(Review::Link));
        assert_eq!(
            needs_review("Write to billing@client-payments.example from now on."),
            Some(Review::Address)
        );
        assert_eq!(
            needs_review("Invoices now go to IBAN CH93 0076 2011 6238 5295 7."),
            Some(Review::BankDetails)
        );
        assert_eq!(
            needs_review("iban de89370400440532013000"),
            Some(Review::BankDetails),
            "in any case, unspaced"
        );
        assert_eq!(
            needs_review("Card 4111 1111 1111 1111, expires 09/29"),
            Some(Review::BankDetails)
        );
        assert_eq!(needs_review("Ref 4111 1111 1111 1112"), None, "fails the Luhn check");
        assert_eq!(Review::BankDetails.describe(), "bank or card details");
    }
}
