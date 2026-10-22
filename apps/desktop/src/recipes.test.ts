import { describe, expect, it } from "vitest";
import { learnVoicePrompt } from "./identities";
import { RECIPES, SAFE_PROMPT, recipeGroups } from "./recipes";

describe("recipes", () => {
  it("are safe to hand an agent on a command line", () => {
    for (const recipe of RECIPES) expect(recipe.prompt, recipe.id).toMatch(SAFE_PROMPT);
    expect(learnVoicePrompt("me@example.org")).toMatch(SAFE_PROMPT);
    expect('say "hi" & leave').not.toMatch(SAFE_PROMPT);
  });

  it("are grouped in order, each id once", () => {
    expect(recipeGroups().map((g) => g.group)).toEqual(["Every day", "Follow-ups", "Your voice", "Tidying"]);
    expect(new Set(RECIPES.map((r) => r.id)).size).toBe(RECIPES.length);
  });
});
