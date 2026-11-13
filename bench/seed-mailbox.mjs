// Fills an empty IMAP mailbox with the benchmark's fixed set of messages, so
// every mail app is measured on the same mail. The set is generated, not
// stored: the same version always gives the same bytes (see the fingerprint
// printed at the end, and bench/README.md).
//
//   node bench/seed-mailbox.mjs --dry-run          # sizes and fingerprint only
//   BENCH_IMAP_HOST=mail.example.org BENCH_IMAP_USER=bench@example.org \
//   BENCH_IMAP_PASSWORD=... node bench/seed-mailbox.mjs
//
// Optional: BENCH_IMAP_PORT (993), BENCH_IMAP_CA (a PEM file, for a test
// server with its own certificate), --messages N (20000), --force (fill a
// mailbox that already holds mail). No dependencies.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import tls from "node:tls";

const CORPUS = "regards-bench-1";
const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const option = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
const TOTAL = Number(option("--messages", 20000));
// The newest message's date. Fixed, so the set does not depend on the day it is made.
const NEWEST = Date.UTC(2026, 9, 1, 8, 0, 0);
const STEP_MS = 23 * 60 * 1000;

/** A small seeded generator (mulberry32): the same seed gives the same numbers everywhere. */
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const WORDS = (
  "invoice meeting project update schedule report budget delivery contract review design launch customer order " +
  "payment travel booking ticket hotel flight offer proposal draft feedback question answer reminder deadline " +
  "document signature agreement renewal subscription account access password security notice change request " +
  "approval team office holiday weekend dinner lunch family photos school parents course lesson training workshop " +
  "conference speaker agenda minutes summary notes plan roadmap release version issue fix test result quality " +
  "supplier warehouse stock shipping parcel tracking return refund receipt statement bank transfer salary tax " +
  "insurance claim policy doctor appointment garage service repair apartment rent lease neighbour building works " +
  "newsletter digest weekly monthly news article event invitation welcome thanks confirmation follow details"
).split(" ");
const FIRST = "Anna Luca Mia Noah Lea Leon Emma Elias Lina Jonas Sara David Nora Simon Julia Marco Laura Felix Clara Tim Eva Paul Nina Max Zoe Jan Iris Tom Lena Ben".split(" ");
const LAST = "Keller Meier Rochat Favre Weber Huber Schmid Moser Baumann Frey Roth Graf Vogel Suter Blanc Girard Martin Bianchi Rossi Monnier".split(" ");
const DOMAINS = "example.test atelier.test bureau.test studio.test fabrik.test verein.test kanzlei.test praxis.test laden.test schule.test".split(" ");
const FOLDERS = [
  { name: "INBOX", share: 0.6 },
  { name: "Archive", share: 0.3 },
  { name: "Sent", share: 0.1 },
];
const ME = "Bench User <bench@regards.test>";

const pick = (rand, list) => list[Math.floor(rand() * list.length)];
const words = (rand, count) => Array.from({ length: count }, () => pick(rand, WORDS)).join(" ");
const sentence = (rand) => {
  const text = words(rand, 6 + Math.floor(rand() * 12));
  return text[0].toUpperCase() + text.slice(1) + ".";
};
const paragraphs = (rand, bytes) => {
  const out = [];
  let size = 0;
  while (size < bytes) {
    const lines = Array.from({ length: 2 + Math.floor(rand() * 4) }, () => sentence(rand)).join("\r\n");
    out.push(lines);
    size += lines.length + 4;
  }
  return out;
};
const person = (rand) => {
  const first = pick(rand, FIRST);
  const last = pick(rand, LAST);
  return `${first} ${last} <${first.toLowerCase()}.${last.toLowerCase()}@${pick(rand, DOMAINS)}>`;
};
const base64 = (buffer) => buffer.toString("base64").replace(/.{76}/g, "$&\r\n");

/** Message number `n` (0 is the oldest) of the set: its folder, flags, date and bytes. */
function message(n) {
  const rand = random(0x5eed0000 + n);
  const roll = rand();
  let folder = FOLDERS[FOLDERS.length - 1].name;
  for (let i = 0, edge = 0; i < FOLDERS.length; i++) {
    edge += FOLDERS[i].share;
    if (roll < edge) {
      folder = FOLDERS[i].name;
      break;
    }
  }
  const date = new Date(NEWEST - (TOTAL - 1 - n) * STEP_MS);
  const sent = folder === "Sent";
  const other = person(rand);
  // Three messages in ten answer one of the fifty before them.
  const parent = n > 50 && rand() < 0.3 ? n - 1 - Math.floor(rand() * 50) : null;
  const subject = parent === null ? words(rand, 3 + Math.floor(rand() * 5)) : `Re: ${words(random(0xabc0000 + parent), 4)}`;
  const kind = rand();
  const headers = [
    `Date: ${date.toUTCString().replace("GMT", "+0000")}`,
    `From: ${sent ? ME : other}`,
    `To: ${sent ? other : ME}`,
    `Subject: ${subject[0].toUpperCase()}${subject.slice(1)}`,
    `Message-ID: <${CORPUS}.${n}@regards.test>`,
    ...(parent === null
      ? []
      : [`In-Reply-To: <${CORPUS}.${parent}@regards.test>`, `References: <${CORPUS}.${parent}@regards.test>`]),
    "MIME-Version: 1.0",
  ];
  const text = paragraphs(rand, 600 + Math.floor(rand() * 7000));
  const plain = text.join("\r\n\r\n") + "\r\n";
  let body;
  if (kind < 0.03) {
    // A file attached: 50 to 400 KB.
    const file = Buffer.alloc(50_000 + Math.floor(rand() * 350_000));
    for (let i = 0; i < file.length; i++) file[i] = Math.floor(rand() * 256);
    const edge = `=_${CORPUS}_${n}`;
    headers.push(`Content-Type: multipart/mixed; boundary="${edge}"`);
    body =
      `--${edge}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n${plain}` +
      `--${edge}\r\nContent-Type: application/pdf; name="document-${n}.pdf"\r\n` +
      `Content-Disposition: attachment; filename="document-${n}.pdf"\r\nContent-Transfer-Encoding: base64\r\n\r\n` +
      `${base64(file)}\r\n--${edge}--\r\n`;
  } else if (kind < 0.08) {
    // A newsletter: text and a long HTML part.
    const blocks = paragraphs(rand, 30_000 + Math.floor(rand() * 30_000));
    const html =
      `<html><body style="font-family:sans-serif"><table width="600" align="center"><tr><td>\r\n` +
      blocks.map((block) => `<h2>${sentence(rand)}</h2>\r\n<p>${block.replace(/\r\n/g, "<br>\r\n")}</p>`).join("\r\n") +
      `\r\n</td></tr></table></body></html>\r\n`;
    const edge = `=_${CORPUS}_${n}`;
    headers.push(`Content-Type: multipart/alternative; boundary="${edge}"`);
    body =
      `--${edge}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n${plain}` +
      `--${edge}\r\nContent-Type: text/html; charset=utf-8\r\n\r\n${html}--${edge}--\r\n`;
  } else {
    headers.push("Content-Type: text/plain; charset=utf-8");
    body = plain;
  }
  const flags = [];
  if (sent || rand() >= 0.2) flags.push("\\Seen");
  if (rand() < 0.02) flags.push("\\Flagged");
  return { folder, flags, date, bytes: Buffer.from(`${headers.join("\r\n")}\r\n\r\n${body}`, "utf8") };
}

/** IMAP's own date form: "01-Oct-2026 08:00:00 +0000". */
function internalDate(date) {
  const months = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split(" ");
  const two = (value) => String(value).padStart(2, "0");
  return (
    `${two(date.getUTCDate())}-${months[date.getUTCMonth()]}-${date.getUTCFullYear()} ` +
    `${two(date.getUTCHours())}:${two(date.getUTCMinutes())}:${two(date.getUTCSeconds())} +0000`
  );
}

/** Just enough IMAP to sign in, make folders and add messages. */
class Imap {
  #socket;
  #pending = "";
  #lines = [];
  #wake = null;
  #tag = 0;

  static async connect(host, port, ca) {
    const imap = new Imap();
    imap.#socket = tls.connect({ host, port, servername: host, ca });
    imap.#socket.setEncoding("latin1");
    imap.#socket.on("data", (chunk) => {
      imap.#pending += chunk;
      let end;
      while ((end = imap.#pending.indexOf("\r\n")) !== -1) {
        imap.#lines.push(imap.#pending.slice(0, end));
        imap.#pending = imap.#pending.slice(end + 2);
      }
      imap.#wake?.();
    });
    await new Promise((resolve, reject) => {
      imap.#socket.once("secureConnect", resolve);
      imap.#socket.once("error", reject);
    });
    imap.#socket.on("error", (error) => {
      console.error(`connection lost: ${error.message}`);
      process.exit(1);
    });
    await imap.#line(); // the greeting
    return imap;
  }

  async #line() {
    while (this.#lines.length === 0) await new Promise((resolve) => (this.#wake = resolve));
    return this.#lines.shift();
  }

  /** Sends one command, with an optional literal after it. Resolves with the untagged lines; rejects unless the answer is OK. */
  async command(text, literal) {
    const tag = `B${++this.#tag}`;
    const seen = [];
    if (literal) {
      this.#socket.write(`${tag} ${text} {${literal.length}}\r\n`);
      for (;;) {
        const line = await this.#line();
        if (line.startsWith("+")) break;
        if (line.startsWith(`${tag} `)) throw new Error(`${text.split(" ")[0]}: ${line}`);
        seen.push(line);
      }
      this.#socket.write(literal);
      this.#socket.write("\r\n");
    } else {
      this.#socket.write(`${tag} ${text}\r\n`);
    }
    for (;;) {
      const line = await this.#line();
      if (line.startsWith(`${tag} OK`)) return seen;
      if (line.startsWith(`${tag} `)) throw new Error(`${text.split(" ")[0]}: ${line}`);
      seen.push(line);
    }
  }

  close() {
    this.#socket.end();
  }
}

const quoted = (text) => `"${text.replace(/[\\"]/g, "\\$&")}"`;

const dryRun = flag("--dry-run");
const host = process.env.BENCH_IMAP_HOST;
const user = process.env.BENCH_IMAP_USER;
const password = process.env.BENCH_IMAP_PASSWORD;
if (!dryRun && !(host && user && password)) {
  console.error("Set BENCH_IMAP_HOST, BENCH_IMAP_USER and BENCH_IMAP_PASSWORD, or pass --dry-run.");
  process.exit(2);
}

let imap = null;
if (!dryRun) {
  const ca = process.env.BENCH_IMAP_CA ? readFileSync(process.env.BENCH_IMAP_CA) : undefined;
  imap = await Imap.connect(host, Number(process.env.BENCH_IMAP_PORT ?? 993), ca);
  await imap.command(`LOGIN ${quoted(user)} ${quoted(password)}`);
  const status = (await imap.command("STATUS INBOX (MESSAGES)")).join(" ");
  const held = Number(status.match(/MESSAGES (\d+)/)?.[1] ?? 0);
  if (held > 0 && !flag("--force")) {
    console.error(`The inbox of ${user} already holds ${held} messages. Use an empty mailbox made for the benchmark, or pass --force.`);
    process.exit(2);
  }
  for (const { name } of FOLDERS) {
    if (name === "INBOX") continue;
    // Already there on most providers.
    await imap.command(`CREATE ${quoted(name)}`).catch(() => {});
  }
}

const fingerprint = createHash("sha256");
const counts = Object.fromEntries(FOLDERS.map(({ name }) => [name, 0]));
let bytes = 0;
const started = Date.now();
for (let n = 0; n < TOTAL; n++) {
  const mail = message(n);
  fingerprint.update(mail.folder).update(mail.flags.join(" ")).update(mail.bytes);
  counts[mail.folder]++;
  bytes += mail.bytes.length;
  if (imap) {
    await imap.command(`APPEND ${quoted(mail.folder)} (${mail.flags.join(" ")}) "${internalDate(mail.date)}"`, mail.bytes);
    if ((n + 1) % 500 === 0) console.log(`${n + 1} of ${TOTAL} (${Math.round((Date.now() - started) / 1000)} s)`);
  }
}
if (imap) {
  await imap.command("LOGOUT").catch(() => {});
  imap.close();
}
console.log(`${CORPUS}: ${TOTAL} messages, ${(bytes / 1048576).toFixed(1)} MB`);
console.log(Object.entries(counts).map(([name, count]) => `${name} ${count}`).join(", "));
console.log(`fingerprint sha256:${fingerprint.digest("hex")}`);
