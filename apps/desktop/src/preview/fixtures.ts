// Sample mail for the UI preview. Every person and company here is made up;
// addresses use the reserved example domains (RFC 2606).

import type { AccountSummary, Attachment, MailboxSummary, Person, Role } from "../accounts";

export interface FixtureMessage {
  id: number;
  mailboxId: number;
  subject: string;
  from: Person;
  to: Person[];
  cc: Person[];
  received_at: number;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  text: string;
  html: string | null;
  remote_images: number;
  attachments: Attachment[];
  /** Messages with the same thread key form a conversation. */
  thread: string;
  pinned?: boolean;
  aside?: boolean;
  snoozed_until?: number | null;
  remind_at?: number | null;
  /** A reminder came due: shown in the inbox from wherever it is. */
  reminded?: boolean;
  /** When it came back to the inbox; listed there by it. */
  surfaced_at?: number | null;
}

export interface FixtureAccount {
  account: AccountSummary;
  mailboxes: MailboxSummary[];
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const person = (name: string, address: string): Person => ({ name, address });

export const ME = person("Sam Rivera", "sam@example.org");
const ME_WORK = person("Sam Rivera", "sam.rivera@example.com");

const lena = person("Lena Okafor", "lena.okafor@example.net");
const marco = person("Marco Bianchi", "marco@example.com");
const priya = person("Priya Natarajan", "priya.n@example.net");
const tomas = person("Tomás Herrera", "tomas@tiles.example");
const hannah = person("Hannah Weiss", "hannah.weiss@example.org");
const yuki = person("Yuki Tanaka", "yuki@example.net");
const elena = person("Elena Petrova", "elena.petrova@example.com");
const daniel = person("Daniel Mensah", "d.mensah@example.net");
const ola = person("Ola Nordmann", "ola@residents.example");
const rail = person("Northline Rail", "tickets@northline.example");
const notes = person("Field Notes", "hello@fieldnotes.example");
const bank = person("Cobalt Bank", "no-reply@cobalt-bank.example");
const parcel = person("Parcel Point", "updates@parcelpoint.example");
const pool = person("Community Pool", "info@pool.example");
const kiln = person("Studio Kiln", "bookings@studiokiln.example");
const prizes = person("Prize Desk", "win@prizes.example");
const grace = person("Grace Liu", "grace.liu@example.com");
const omar = person("Omar Haddad", "omar.haddad@example.com");
const ines = person("Inès Moreau", "ines.moreau@example.com");

function account(id: string, me: Person, created: number): AccountSummary {
  const host = me.address?.split("@")[1] ?? "example.org";
  return {
    id,
    display_name: me.name ?? "",
    email: me.address ?? "",
    username: me.address ?? "",
    imap: { host: `mail.${host}`, port: 993, security: "tls" },
    smtp: { host: `mail.${host}`, port: 587, security: "starttls" },
    imap_capabilities: ["IMAP4rev1", "IDLE", "UIDPLUS", "MOVE", "CONDSTORE", "QRESYNC"],
    created_at: created,
  };
}

const box = (id: number, name: string, role: Role | null, selectable = true): MailboxSummary => ({
  id,
  name,
  delimiter: "/",
  role,
  selectable,
});

export const PERSONAL: FixtureAccount = {
  account: account("personal", ME, Date.now() - 400 * DAY),
  mailboxes: [
    box(1, "INBOX", "inbox"),
    box(2, "Drafts", "drafts"),
    box(3, "Sent", "sent"),
    box(4, "Archive", "archive"),
    box(5, "Junk", "junk"),
    box(6, "Trash", "trash"),
    box(7, "Projects", null),
    box(8, "Projects/Garden", null),
    box(9, "Projects/Renovation", null),
    box(10, "Receipts", null),
    box(11, "Travel", null),
  ],
};

export const WORK: FixtureAccount = {
  account: account("work", ME_WORK, Date.now() - 200 * DAY),
  mailboxes: [
    box(21, "INBOX", "inbox"),
    box(22, "Sent Items", "sent"),
    box(23, "Archive", "archive"),
    box(24, "Deleted Items", "trash"),
    box(25, "Clients", null),
  ],
};

const pdf = (name: string, size: number): Attachment => ({ name, mime: "application/pdf", size, inline: false });
const jpg = (name: string, size: number): Attachment => ({ name, mime: "image/jpeg", size, inline: false });

const NEWSLETTER = `<!doctype html><html><body style="margin:0;background:#f3efe7;font-family:Georgia,serif;color:#2b2722">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr><td align="center" style="padding:28px 12px">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="max-width:600px;background:#fffdf8;border-radius:10px">
<tr><td style="padding:28px 36px 8px;font:600 12px/1.4 Helvetica,Arial,sans-serif;letter-spacing:.12em;text-transform:uppercase;color:#9a6b3f">Field Notes · Issue 142</td></tr>
<tr><td style="padding:0 36px"><h1 style="margin:6px 0 14px;font-size:30px;line-height:1.2;font-weight:normal">The quiet return of the night train</h1>
<p style="font-size:17px;line-height:1.6;margin:0 0 16px">Ten years ago most sleeper routes across the continent looked finished. This winter there are more of them than at any time since the eighties, and the people who ride them are not who you might expect.</p>
<div style="height:180px;border-radius:8px;background:linear-gradient(135deg,#27435f,#d08a4a);margin:8px 0 18px"></div>
<p style="font-size:17px;line-height:1.6;margin:0 0 16px">We spent a week on the rails between the Baltic and the Adriatic, sharing compartments with students, retirees and one cellist who had booked the same berth every month for a year.</p>
<p style="margin:22px 0 30px"><a href="https://fieldnotes.example/142" style="background:#2b2722;color:#fffdf8;padding:12px 20px;border-radius:6px;text-decoration:none;font:600 14px Helvetica,Arial,sans-serif">Read the full story</a></p>
</td></tr>
<tr><td style="padding:18px 36px 28px;border-top:1px solid #eee4d6;font:13px/1.5 Helvetica,Arial,sans-serif;color:#8a8073">Also this week: a field guide to winter markets, and readers' letters on slow travel.<br>You are receiving this because you subscribed at fieldnotes.example.</td></tr>
</table></td></tr></table></body></html>`;

const TICKETS = `<!doctype html><html><body style="margin:0;background:#eef2f6;font-family:Helvetica,Arial,sans-serif;color:#17212b">
<div style="max-width:560px;margin:24px auto;background:#fff;border-radius:12px;overflow:hidden">
<div style="background:#123b63;color:#fff;padding:22px 28px;font-size:20px;font-weight:700">Northline Rail</div>
<div style="padding:24px 28px">
<p style="font-size:16px;margin:0 0 18px">Hello Sam, your booking is confirmed. Your tickets are attached to this email.</p>
<table style="width:100%;border-collapse:collapse;font-size:15px">
<tr><td style="padding:10px 0;color:#5b6b7b">Outbound</td><td style="padding:10px 0;text-align:right;font-weight:700">Zürich HB → Lugano</td></tr>
<tr><td style="padding:10px 0;color:#5b6b7b;border-top:1px solid #e6ebf0">Date</td><td style="padding:10px 0;text-align:right;border-top:1px solid #e6ebf0">Sat 24 Oct · 08:09 – 10:16</td></tr>
<tr><td style="padding:10px 0;color:#5b6b7b;border-top:1px solid #e6ebf0">Seats</td><td style="padding:10px 0;text-align:right;border-top:1px solid #e6ebf0">Coach 7 · 64, 66</td></tr>
<tr><td style="padding:10px 0;color:#5b6b7b;border-top:1px solid #e6ebf0">Booking reference</td><td style="padding:10px 0;text-align:right;border-top:1px solid #e6ebf0;font-family:monospace;font-size:16px">NLR-7Q4K2</td></tr>
</table>
<p style="font-size:13px;color:#5b6b7b;margin:22px 0 0">Have a good journey.</p>
</div></div></body></html>`;

interface Spec {
  mailbox: number;
  ago: number;
  from: Person;
  to?: Person[];
  cc?: Person[];
  subject: string;
  text: string;
  seen?: boolean;
  flagged?: boolean;
  answered?: boolean;
  html?: string;
  remote?: number;
  files?: Attachment[];
  thread?: string;
  pinned?: boolean;
  aside?: boolean;
}

const SPECS: Spec[] = [
  // Inbox
  {
    mailbox: 1,
    ago: 12 * MINUTE,
    from: lena,
    subject: "Re: Garden plot schedule",
    text: "Perfect, Saturdays work for us too. I'll bring the seedlings for the north beds and the spare hose.\n\nShall we put the rota on the shed door so the others can add themselves?\n\nLena",
    seen: false,
    thread: "garden",
  },
  {
    mailbox: 1,
    ago: 48 * MINUTE,
    from: rail,
    subject: "Your tickets: Zürich → Lugano, Sat 24 Oct",
    text: "Hello Sam, your booking is confirmed. Outbound: Zürich HB → Lugano, Sat 24 Oct, 08:09 – 10:16. Coach 7, seats 64 and 66. Booking reference NLR-7Q4K2.",
    html: TICKETS,
    seen: false,
    files: [pdf("Tickets-NLR-7Q4K2.pdf", 182_400)],
  },
  {
    mailbox: 1,
    ago: 2 * HOUR + 10 * MINUTE,
    from: marco,
    subject: "Photos from Sunday",
    text: "Here are the best ones from the lake. The one with the boat came out better than I expected!\n\nMarco",
    seen: false,
    files: [jpg("lake-01.jpg", 2_410_000), jpg("lake-02.jpg", 1_980_000), jpg("boat.jpg", 3_120_000)],
  },
  {
    mailbox: 1,
    ago: 3 * HOUR + 30 * MINUTE,
    from: notes,
    subject: "Field Notes #142: The quiet return of the night train",
    text: "The quiet return of the night train. Ten years ago most sleeper routes across the continent looked finished…",
    html: NEWSLETTER,
    remote: 4,
  },
  {
    mailbox: 1,
    ago: 5 * HOUR,
    from: priya,
    subject: "Dinner on Thursday?",
    pinned: true,
    text: "Hi Sam,\n\nAre you free on Thursday evening? We're trying the new place by the river around 19:30. Bring Lena if she's around!\n\nPriya",
    flagged: true,
    answered: true,
    thread: "dinner",
  },
  {
    mailbox: 1,
    ago: 7 * HOUR,
    from: bank,
    subject: "Your October statement is ready",
    text: "Your statement for the account ending 4821 is ready to view in online banking.",
  },
  {
    mailbox: 1,
    ago: 26 * HOUR,
    from: hannah,
    subject: "Book club: next pick",
    text: "Three options for November, vote by Friday:\n\n1. A novel about a lighthouse keeper\n2. The essay collection Yuki recommended\n3. Something short, since December is busy\n\nHannah",
    cc: [yuki, priya],
  },
  {
    mailbox: 1,
    ago: 29 * HOUR,
    from: tomas,
    subject: "Re: Quote for the bathroom tiles",
    pinned: true,
    text: "Hello Sam,\n\nAs promised, the quote for the tiles and the labour is attached. The matte grey you liked is in stock until the end of the month.\n\nBest regards,\nTomás Herrera",
    seen: false,
    flagged: true,
    files: [pdf("Quote-2026-114.pdf", 96_200)],
    thread: "tiles",
  },
  {
    mailbox: 1,
    ago: 2 * DAY + 3 * HOUR,
    from: yuki,
    subject: "Re: Japanese lessons",
    text: "Tuesdays at 18:00 are fine. I'll send you the first worksheet before then.",
    answered: true,
  },
  {
    mailbox: 1,
    ago: 2 * DAY + 6 * HOUR,
    from: parcel,
    subject: "Your parcel is out for delivery",
    text: "Your parcel 3S-88213 will be delivered today between 10:00 and 13:00.",
  },
  { mailbox: 1, ago: 4 * DAY, from: pool, subject: "Winter opening hours", aside: true, text: "From 2 November the pool opens at 07:00 on weekdays." },
  {
    mailbox: 1,
    ago: 5 * DAY,
    from: elena,
    subject: "Invitation: Anna's 40th",
    text: "We're throwing Anna a surprise party on the 14th. Not a word to her! Details to follow.",
    seen: false,
  },
  { mailbox: 1, ago: 6 * DAY, from: kiln, subject: "Your pottery class is confirmed", aside: true, text: "See you on Wednesday at 19:00. Wear clothes that can get muddy." },
  { mailbox: 1, ago: 8 * DAY, from: daniel, subject: "Bike for sale — still available?", text: "Hi, is the city bike still for sale? I could come by on the weekend." },
  {
    mailbox: 1,
    ago: 9 * DAY,
    from: ola,
    subject: "Minutes from the residents' meeting",
    text: "Attached are the minutes. The main points: new bike racks, the courtyard lights and the spring clean-up date.",
    files: [{ name: "Minutes-October.docx", mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", size: 48_300, inline: false }],
  },
  { mailbox: 1, ago: 12 * DAY, from: bank, subject: "Security notice: new sign-in", text: "We noticed a new sign-in to online banking from a new device." },
  { mailbox: 1, ago: 14 * DAY, from: marco, subject: "Re: Hiking in the Ticino", text: "The route via the ridge is longer but the views are worth it." },
  { mailbox: 1, ago: 18 * DAY, from: priya, subject: "The recipe you asked for", text: "Here it is. The secret is to toast the spices first." },

  // Sent
  {
    mailbox: 3,
    ago: 2 * DAY,
    from: ME,
    to: [lena],
    subject: "Re: Garden plot schedule",
    text: "Saturday mornings suit me best. I can take the watering on Wednesdays as well.\n\nSam",
    thread: "garden",
  },
  {
    mailbox: 3,
    ago: 4 * HOUR,
    from: ME,
    to: [priya],
    subject: "Re: Dinner on Thursday?",
    text: "Yes please! I'll ask Lena too. See you there.",
    thread: "dinner",
  },
  {
    mailbox: 3,
    ago: 4 * DAY,
    from: ME,
    to: [tomas],
    subject: "Quote for the bathroom tiles",
    text: "Hello Tomás,\n\nCould you send me a quote for the matte grey tiles we discussed, including labour?\n\nThank you,\nSam",
    thread: "tiles",
  },

  // Archive
  {
    mailbox: 4,
    ago: 3 * DAY,
    from: lena,
    subject: "Garden plot schedule",
    text: "Hi all,\n\nNow that the plot is ours for another year, let's agree on who looks after it when. Which days suit you?\n\nLena",
    thread: "garden",
  },
  { mailbox: 4, ago: 30 * DAY, from: kiln, subject: "Welcome to Studio Kiln", text: "Thank you for joining." },

  // Junk and Trash
  { mailbox: 5, ago: 6 * HOUR, from: prizes, subject: "You have been selected for a FREE cruise!!!", text: "Claim now." },
  { mailbox: 5, ago: 3 * DAY, from: prizes, subject: "Final notice: your prize is waiting", text: "Claim now." },
  { mailbox: 6, ago: 10 * DAY, from: parcel, subject: "Rate your delivery", text: "How did we do?" },

  // Folders
  { mailbox: 8, ago: 20 * DAY, from: lena, subject: "Seed order list", text: "Tomatoes, beans, two kinds of kale." },
  { mailbox: 9, ago: 25 * DAY, from: tomas, subject: "Tile samples", text: "The samples are ready to pick up." },
  { mailbox: 10, ago: 15 * DAY, from: kiln, subject: "Receipt for your class pack", text: "Thank you for your payment." },
  { mailbox: 11, ago: 40 * DAY, from: rail, subject: "Your trip to Basel", text: "Booking reference NLR-2M8P1." },

  // Work
  {
    mailbox: 21,
    ago: 35 * MINUTE,
    from: grace,
    to: [ME_WORK],
    subject: "Q4 planning — agenda for Monday",
    text: "Hi team, the agenda is below. Please add anything by Friday noon.",
    seen: false,
  },
  {
    mailbox: 21,
    ago: 4 * HOUR,
    from: omar,
    to: [ME_WORK],
    subject: "Design review notes",
    text: "Thanks for the walkthrough. My notes are in the shared doc; the main open question is the onboarding flow.",
  },
  { mailbox: 21, ago: 27 * HOUR, from: ines, to: [ME_WORK], subject: "Contract renewal", text: "The renewal is ready for signature.", flagged: true },
  { mailbox: 25, ago: 9 * DAY, from: ines, to: [ME_WORK], subject: "Kick-off recap", text: "Recap of the kick-off attached." },
];

/** Older mail, so the list has pages to scroll through. */
function older(): Spec[] {
  const senders = [notes, bank, pool, kiln, parcel, marco, hannah, daniel];
  const subjects = [
    "Weekly digest",
    "Your receipt",
    "Reminder: upcoming booking",
    "Photos",
    "Quick question",
    "Newsletter",
    "Order confirmation",
    "Catching up",
  ];
  return Array.from({ length: 90 }, (_, index) => ({
    mailbox: 1,
    ago: (20 + index * 2.3) * DAY,
    from: senders[index % senders.length],
    subject: `${subjects[(index * 3) % subjects.length]} (${index + 1})`,
    text: "An older message, here so the list has more pages.",
  }));
}

export function fixtureMessages(now: number, accounts: FixtureAccount[]): FixtureMessage[] {
  const mailboxIds = new Set(accounts.flatMap((a) => a.mailboxes.map((m) => m.id)));
  return [...SPECS, ...older()]
    .filter((spec) => mailboxIds.has(spec.mailbox))
    .map((spec, index) => ({
      id: index + 1,
      mailboxId: spec.mailbox,
      subject: spec.subject,
      from: spec.from,
      to: spec.to ?? [spec.mailbox >= 21 ? ME_WORK : ME],
      cc: spec.cc ?? [],
      received_at: now - spec.ago,
      seen: spec.seen ?? true,
      flagged: spec.flagged ?? false,
      answered: spec.answered ?? false,
      text: spec.text,
      html: spec.html ?? null,
      remote_images: spec.remote ?? 0,
      attachments: spec.files ?? [],
      thread: spec.thread ?? `single-${index}`,
      pinned: spec.pinned ?? false,
      aside: spec.aside ?? false,
    }));
}
