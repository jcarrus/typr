import { equal, ok } from "node:assert/strict";
import {
  compactApplicationText,
  createRewriteRequest,
  withRewriteLifetime,
} from "./typr.ts";

const settings = {
  openAIKey: "",
  llmPrompt: "",
  useLocalWhisper: true,
  whisperPrompt: "",
  whisperKitModel: "small.en_217MB",
  userProfile: "",
  customTerms: ["OverAI"],
  qwenModel: "qwen3.5:4b",
  profileModel: "qwen3.6:latest",
  showCompletionNotification: true,
};
const requestFor = (visibleText: string) =>
  createRewriteRequest("Look at Dreamer OS.", { visibleText }, settings, {
    version: 1,
    rules: [{ id: "test", text: "Justin works at OverAI." }],
  });

Deno.test("compacts decorative rules and blank lines without changing words or indentation", () => {
  equal(
    compactApplicationText(
      `DreamerOS   \n${"─".repeat(200)} title ${
        "=".repeat(80)
      }\n\n\n\n    code... 1000000000 aaaaaaaa`,
    ),
    "DreamerOS\n─── title ===\n\n    code... 1000000000 aaaaaaaa",
  );
});

Deno.test("compacts before applying the 10,000-character limit", () => {
  equal(
    compactApplicationText(`DreamerOS\n${"─".repeat(12_000)}\nEnd`),
    "DreamerOS\n───\nEnd",
  );
  const text = "abcdefghij".repeat(1_100);
  equal(compactApplicationText(text), text.slice(-10_000));
});

Deno.test("keeps product names beyond the old 1,200-character tail", () => {
  const request = requestFor(
    `Founder of DreamerOS.\n${"Other context. ".repeat(250)}`,
  );
  const context = request.prompt.split("<focused-application-text>\n")[1]
    .split("</focused-application-text>")[0];
  ok(context.includes("DreamerOS"));
  equal(request.options.num_ctx, 16_384);
});

Deno.test("orders instructions, profile, Accessibility text, then dictation", () => {
  const request = requestFor("Reference text");
  const sections = [
    "Copyediting instructions:",
    "Profile rules",
    "Terminology:",
    "Application context:",
    "<focused-application-text>",
    "<dictation>",
  ].map((section) => request.prompt.indexOf(section));
  ok(
    sections.every((position, i) =>
      position >= 0 && (i === 0 || position > sections[i - 1])
    ),
  );
  ok(request.prompt.endsWith("Look at Dreamer OS.\n</dictation>"));
});

Deno.test("uses caret text when visible text is unavailable", () => {
  const request = createRewriteRequest(
    "Hello.",
    {
      textBeforeCursor: "Before",
      selectedText: "Selected",
      textAfterCursor: "After",
    },
    settings,
    { version: 1, rules: [] },
  );
  ok(request.prompt.includes("Before\nSelected\nAfter"));
  ok(requestFor("").prompt.includes("\nUnavailable\n"));
});

Deno.test("rewrite requests expire after one idle hour, including archived requests", () => {
  const request = requestFor("Reference text");
  equal(request.keep_alive, 3_600);
  const archived = { ...request, keep_alive: -1 };
  equal(withRewriteLifetime(archived).keep_alive, 3_600);
  equal(archived.keep_alive, -1);
});

Deno.test("screen changes leave the instruction and profile prefix unchanged", () => {
  const first = requestFor("First screen").prompt;
  const second = requestFor("Different screen").prompt;
  equal(
    first.split("<focused-application-text>")[0],
    second.split("<focused-application-text>")[0],
  );
  ok(first.indexOf("Examples:") < first.indexOf("Application context:"));
});
