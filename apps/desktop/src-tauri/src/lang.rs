//! What the shell writes itself (the tray, notifications, dialogs) in the
//! window's language: English, or French once the window says so
//! (Settings › General › Language). Kept with the other desktop settings,
//! so the tray speaks it before the window opens.

use std::sync::atomic::{AtomicBool, Ordering};

static FRENCH: AtomicBool = AtomicBool::new(false);

/// The window's language: French, or English for anything else.
pub(crate) fn set(language: &str) {
    FRENCH.store(language == "fr", Ordering::SeqCst);
}

fn french() -> bool {
    FRENCH.load(Ordering::SeqCst)
}

/// `english` in the shell's language.
pub(crate) fn tr(english: &'static str) -> &'static str {
    translate(english, french())
}

/// `english` with its `{name}`s filled in, in the shell's language.
pub(crate) fn tr_with(english: &'static str, values: &[(&str, &str)]) -> String {
    fill(tr(english), values)
}

/// A count's text: `one` (English, for 1) or `many` (English, the key),
/// as the shell's language counts (in French, 0 and 1 take the first);
/// `{count}` is the number.
pub(crate) fn count(n: usize, one: &'static str, many: &'static str) -> String {
    counted(n, one, many, french())
}

fn translate(english: &'static str, french: bool) -> &'static str {
    if !french {
        return english;
    }
    FR.iter()
        .find(|(key, _)| *key == english)
        .map_or(english, |(_, translated)| translated)
}

fn counted(n: usize, one: &'static str, many: &'static str, french: bool) -> String {
    let singular = if french { n <= 1 } else { n == 1 };
    let form = if singular { one } else { many };
    fill(translate(form, french), &[("count", &n.to_string())])
}

fn fill(text: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(text.to_owned(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), value)
    })
}

/// The shell's French: each English text and its translation. French
/// typography: a narrow no-break space before ? ! ; and a no-break space
/// before :.
const FR: &[(&str, &str)] = &[
    // The tray.
    ("Open Regards", "Ouvrir Regards"),
    ("Run in the background", "Tourner en arrière-plan"),
    ("Show notifications", "Afficher les notifications"),
    ("Quit Regards", "Quitter Regards"),
    ("Regards is still running", "Regards tourne toujours"),
    (
        "It stays in the tray so snoozed mail comes back and scheduled messages go. Quit it from the tray icon.",
        "Il reste dans la zone de notification pour que les messages reportés reviennent et que les envois programmés partent. Quittez-le depuis son icône.",
    ),
    (
        "It stays in the menu bar so snoozed mail comes back and scheduled messages go. Quit it from the menu bar icon.",
        "Il reste dans la barre des menus pour que les messages reportés reviennent et que les envois programmés partent. Quittez-le depuis son icône.",
    ),
    // Quitting with work waiting.
    ("Quit Regards?", "Quitter Regards\u{202f}?"),
    ("Quit", "Quitter"),
    ("Keep running", "Continuer"),
    ("1 message to send", "{count} message à envoyer"),
    ("{count} messages to send", "{count} messages à envoyer"),
    ("1 snoozed message", "{count} message reporté"),
    ("{count} snoozed messages", "{count} messages reportés"),
    ("1 reminder", "{count} rappel"),
    ("{count} reminders", "{count} rappels"),
    (
        "{list} is waiting. Regards sends messages and brings mail back only while it runs: after you quit, they wait until you open it again.",
        "{list} attend. Regards n’envoie les messages et ne ramène le courrier que pendant qu’il tourne\u{a0}: après l’avoir quitté, ils attendent sa réouverture.",
    ),
    (
        "{list} are waiting. Regards sends messages and brings mail back only while it runs: after you quit, they wait until you open it again.",
        "{list} attendent. Regards n’envoie les messages et ne ramène le courrier que pendant qu’il tourne\u{a0}: après l’avoir quitté, ils attendent sa réouverture.",
    ),
    ("{first} and {last}", "{first} et {last}"),
    // Notifications.
    ("(no subject)", "(sans objet)"),
    ("Back from snooze", "De retour après report"),
    ("No reply yet", "Toujours sans réponse"),
    ("To {who}: {subject}", "À {who}\u{a0}: {subject}"),
    ("Waiting in the Outbox", "En attente dans la boîte d’envoi"),
    (
        "“{subject}” was due while this computer was off. Send it now, or edit it.",
        "«\u{a0}{subject}\u{a0}» devait partir pendant que cet ordinateur était éteint. Envoyez-le maintenant, ou modifiez-le.",
    ),
    ("Not sent", "Non envoyé"),
    (
        "“{subject}” to {who}. The Outbox says why.",
        "«\u{a0}{subject}\u{a0}» à {who}. La boîte d’envoi dit pourquoi.",
    ),
    ("It may not have been sent", "Il n’a peut-être pas été envoyé"),
    (
        "“{subject}” to {who}. Check before sending it again.",
        "«\u{a0}{subject}\u{a0}» à {who}. Vérifiez avant de le renvoyer.",
    ),
    ("An agent is waiting for you", "Un agent vous attend"),
    (
        "{subject}. Approve or reject it in Settings › Agents.",
        "{subject}. Approuvez ou refusez dans Paramètres › Agents.",
    ),
    ("An agent is sending a message", "Un agent envoie un message"),
    (
        "“{subject}” to {who}. Call it back from the Outbox before it goes.",
        "«\u{a0}{subject}\u{a0}» à {who}. Rappelez-le depuis la boîte d’envoi avant son départ.",
    ),
    ("1 new message", "{count} nouveau message"),
    ("{count} new messages", "{count} nouveaux messages"),
    ("1 back from snooze", "{count} de retour après report"),
    ("{count} back from snooze", "{count} de retour après report"),
    (
        "1 message waiting in the Outbox",
        "{count} message en attente dans la boîte d’envoi",
    ),
    (
        "{count} messages waiting in the Outbox",
        "{count} messages en attente dans la boîte d’envoi",
    ),
    ("1 message not sent", "{count} message non envoyé"),
    ("{count} messages not sent", "{count} messages non envoyés"),
    ("1 agent request waiting for you", "{count} demande d’agent vous attend"),
    (
        "{count} agent requests waiting for you",
        "{count} demandes d’agents vous attendent",
    ),
    ("1 message an agent is sending", "{count} message qu’un agent envoie"),
    (
        "{count} messages an agent is sending",
        "{count} messages qu’un agent envoie",
    ),
    ("{one} and {two}", "{one} et {two}"),
    ("{one} and {count} others", "{one} et {count} autres"),
    // Updates.
    ("Regards {version} is available", "Regards {version} est disponible"),
    (
        "Open Settings › About to download it.",
        "Ouvrez Paramètres › À propos pour la télécharger.",
    ),
    // File dialogs.
    ("Save attachment", "Enregistrer la pièce jointe"),
    ("Save all attachments in", "Enregistrer toutes les pièces jointes dans"),
    ("Attach files", "Joindre des fichiers"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_stays_and_french_is_found() {
        assert_eq!(translate("Open Regards", false), "Open Regards");
        assert_eq!(translate("Open Regards", true), "Ouvrir Regards");
        assert_eq!(translate("Something new", true), "Something new");
    }

    #[test]
    fn counts_are_counted_as_each_language_does() {
        assert_eq!(counted(1, "1 reminder", "{count} reminders", false), "1 reminder");
        assert_eq!(counted(0, "1 reminder", "{count} reminders", false), "0 reminders");
        assert_eq!(counted(0, "1 reminder", "{count} reminders", true), "0 rappel");
        assert_eq!(counted(3, "1 reminder", "{count} reminders", true), "3 rappels");
    }

    #[test]
    fn every_french_text_keeps_the_names_of_its_english_one() {
        let names = |text: &str| {
            let mut found: Vec<String> = text
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}').map(|(name, _)| name.to_owned()))
                .collect();
            found.sort();
            found
        };
        for (english, french) in FR {
            let wanted: Vec<String> = names(english).into_iter().filter(|name| name != "count").collect();
            let used: Vec<String> = names(french).into_iter().filter(|name| name != "count").collect();
            assert_eq!(wanted, used, "{english} → {french}");
        }
    }
}
