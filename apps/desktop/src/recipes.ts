import type { IconName } from "./icons";
import { t } from "./i18n";

/**
 * Recipes: ready-made requests to hand an AI agent connected to Regards
 * (the sidebar's Agents menu). The agent does the work through Regards'
 * tools, within what each account allows it; Regards itself sends nothing
 * to any AI.
 *
 * The prompts are kept to letters, digits and plain punctuation (no quotes
 * or shell symbols), because they may be passed to an agent on a command
 * line: see `SAFE_PROMPT`.
 */
export interface Recipe {
  id: string;
  group: string;
  title: string;
  /** What it gives, in a line. */
  caption: string;
  icon: IconName;
  prompt: string;
}

/** What a prompt may hold, to go safely on any command line. */
export const SAFE_PROMPT = /^[\p{L}\p{N} .,:;?()/@\-\n]+$/u;

export const RECIPES: Recipe[] = [
  {
    id: "morning",
    group: t("Every day"),
    title: t("Morning briefing"),
    caption: t("What came in overnight, and what needs a reply before noon."),
    icon: "sun",
    prompt:
      "In Regards, give me my morning briefing. Look at what came into my inboxes since yesterday evening, and group it: " +
      "needs a reply before noon, can wait, for information only. One line per message, with who, what, and its " +
      "regards link. Change nothing.",
  },
  {
    id: "evening",
    group: t("Every day"),
    title: t("End of day"),
    caption: t("What happened today, what is still open, what comes first tomorrow."),
    icon: "moon",
    prompt:
      "In Regards, sum up my day: what came in and what I sent today, what is still open, and what I should do first " +
      "tomorrow. One line per item, with its regards link. Change nothing.",
  },
  {
    id: "needs-reply",
    group: t("Follow-ups"),
    title: t("Needs my reply"),
    caption: t("People who asked you something and are still waiting."),
    icon: "reply",
    prompt:
      "In Regards, find the messages from people in the last 14 days that still wait for my answer: they ask me " +
      "something and I have not replied. Oldest first, with who, what they need, and the regards link. Change nothing.",
  },
  {
    id: "no-answer",
    group: t("Follow-ups"),
    title: t("Still no answer"),
    caption: t("What you asked for that nobody has answered yet."),
    icon: "sendLater",
    prompt:
      "In Regards, find what I sent in the last 14 days that asks for something and that nobody has answered. Oldest " +
      "first, with to whom, what I asked, and the regards link. Then offer to draft a short follow-up for each, in my " +
      "voice. Send nothing.",
  },
  {
    id: "promised",
    group: t("Follow-ups"),
    title: t("What I promised"),
    caption: t("Your commitments in sent mail, and their dates."),
    icon: "check",
    prompt:
      "In Regards, list what I promised in the emails I sent in the last 30 days: what, to whom, and by when. Mark " +
      "what looks overdue, and give the regards link of each. Change nothing.",
  },
  {
    id: "voice",
    group: t("Your voice"),
    title: t("Learn how I write"),
    caption: t("Reads what you sent and suggests your voice, for your approval."),
    icon: "quote",
    prompt:
      "Learn how I write from my sent mail in Regards and suggest it as my voice, for each of my addresses open to " +
      "you, following the Regards skill. Describe how I write, never what I wrote about.",
  },
  {
    id: "newsletters",
    group: t("Tidying"),
    title: t("Quiet the newsletters"),
    caption: t("The senders you never read, to archive once you say so."),
    icon: "newspaper",
    prompt:
      "In Regards, find the newsletters and notifications in my inboxes that I never open. Show me the senders, with " +
      "how many messages each, and ask me which to archive. Archive only what I confirm.",
  },
];

/** The recipes by group, in order. */
export function recipeGroups(): { group: string; recipes: Recipe[] }[] {
  const groups: { group: string; recipes: Recipe[] }[] = [];
  for (const recipe of RECIPES) {
    const last = groups[groups.length - 1];
    if (last?.group === recipe.group) last.recipes.push(recipe);
    else groups.push({ group: recipe.group, recipes: [recipe] });
  }
  return groups;
}
