//! Whatever a message contains, reading it must not crash (plan §5.2: "no
//! panics on malformed input"). Real messages, mangled many ways by a seeded
//! generator, go through every parser the daemon runs on mail.

use regards_core::{body, headers, html};
use std::panic::{AssertUnwindSafe, catch_unwind};

const ROUNDS: u32 = 3_000;

const SAMPLES: &[&[u8]] = &[
    b"From: =?UTF-8?B?w4lsb2TDqWU=?= <e@example.test>\r\nTo: a@example.test, \"B, C\" <b@example.test>\r\n\
      Subject: =?ISO-8859-1?Q?R=E9union?= =?UTF-8?Q?_demain?=\r\nDate: Sun, 18 Oct 2026 10:00:00 +0200\r\n\
      Message-ID: <a@example.test>\r\nIn-Reply-To: <b@example.test>\r\nReferences: <c@example.test> <b@example.test>\r\n\
      \r\nHello\r\n",
    b"MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
      --b\r\nContent-Type: multipart/alternative; boundary=\"a\"\r\n\r\n\
      --a\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nH=C3=A9llo=\r\n world\r\n\
      --a\r\nContent-Type: text/html; charset=windows-1252\r\n\r\n<p onclick=x()>H\xe9llo <img src=\"cid:i@x\"><a href=\"https://e.test/?a=1&amp;b\">l</a></p>\r\n\
      --a--\r\n\
      --b\r\nContent-Type: image/png\r\nContent-ID: <i@x>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n\
      --b\r\nContent-Type: application/pdf; name=\"r.pdf\"\r\nContent-Disposition: attachment; filename*=UTF-8''r%C3%A9.pdf\r\n\
      Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n",
    b"Content-Type: text/html\r\n\r\n<html><head><style>p{color:red}</style><base href=\"https://evil.test/\"></head>\
      <body><table background=\"http://t.test/bg\"><tr><td><img src=\"https://t.test/p.gif\"><script>x()</script>\
      <iframe src=\"https://e.test\"></iframe><form><input></form></td></tr></table></body></html>\r\n",
    b"Content-Type: message/rfc822\r\n\r\nSubject: inner\r\nContent-Type: text/plain\r\n\r\ninner body\r\n",
];

/// xorshift64*: deterministic, so a failure can be replayed by its round.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Damage a message: flip, drop, insert, repeat or cut bytes.
fn mangle(rng: &mut Rng, source: &[u8]) -> Vec<u8> {
    const NASTY: &[&[u8]] = &[
        b"\r\n",
        b"\r\n\r\n",
        b"--b\r\n",
        b"--b--",
        b"=?",
        b"?=",
        b"=?UTF-8?B?",
        b"=?x-unknown?Q?",
        b"boundary=\"\"",
        b"Content-Transfer-Encoding: base64\r\n",
        b"Content-Type: multipart/mixed; boundary=b\r\n",
        b"<",
        b">",
        b"&#xFFFFFFFF;",
        b"\xff\xfe",
        b"\x00",
        b"%",
        b"=\r\n",
    ];
    let mut bytes = source.to_vec();
    for _ in 0..=rng.below(8) {
        let at = rng.below(bytes.len() + 1);
        match rng.below(6) {
            0 if !bytes.is_empty() => {
                let i = at.min(bytes.len() - 1);
                bytes[i] = rng.next() as u8;
            }
            1 if at < bytes.len() => {
                let end = (at + rng.below(32)).min(bytes.len());
                bytes.drain(at..end);
            }
            2 => {
                let nasty = NASTY[rng.below(NASTY.len())];
                bytes.splice(at..at, nasty.iter().copied());
            }
            3 if at < bytes.len() => {
                let end = (at + rng.below(64)).min(bytes.len());
                let chunk = bytes[at..end].to_vec();
                for _ in 0..rng.below(20) {
                    bytes.splice(at..at, chunk.iter().copied());
                }
            }
            4 => bytes.truncate(at),
            _ => {
                let noise: Vec<u8> = (0..rng.below(16)).map(|_| rng.next() as u8).collect();
                bytes.splice(at..at, noise);
            }
        }
    }
    bytes
}

#[test]
fn mangled_messages_never_crash_a_parser() {
    let mut rng = Rng(0x5EED_0F4E_6A4D_5EED);
    for round in 0..ROUNDS {
        let source = SAMPLES[round as usize % SAMPLES.len()];
        let message = mangle(&mut rng, source);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let _ = headers::parse(&message);
            let _ = body::parse(&message);
            let _ = html::render(&message, false, None);
            let _ = html::render(&message, true, Some("regards-mail://localhost/open?t=k&url="));
        }));
        if outcome.is_err() {
            panic!(
                "round {round} crashed a parser on {:?}",
                String::from_utf8_lossy(&message)
            );
        }
    }
}

#[test]
fn degenerate_inputs_never_crash_a_parser() {
    let deep_multipart: Vec<u8> = (0..500)
        .flat_map(|n| format!("Content-Type: multipart/mixed; boundary=\"b{n}\"\r\n\r\n--b{n}\r\n").into_bytes())
        .collect();
    let deep_html = format!("Content-Type: text/html\r\n\r\n{}x", "<div>".repeat(20_000));
    let long_header = format!("Subject: {}\r\n\r\n", "a".repeat(1_000_000));
    let inputs: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"\r\n".to_vec(),
        vec![0; 10_000],
        vec![0xff; 10_000],
        deep_multipart,
        deep_html.into_bytes(),
        long_header.into_bytes(),
        b"Content-Type: text/html\r\nContent-Transfer-Encoding: base64\r\n\r\n!!!not base64!!!".to_vec(),
    ];
    for input in &inputs {
        let _ = headers::parse(input);
        let _ = body::parse(input);
        let _ = html::render(input, false, None);
        let _ = html::render(input, true, Some("p"));
    }
}
