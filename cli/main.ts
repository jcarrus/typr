import { Config } from "./types.ts";

const audioFile = "temp_recording.wav";
const outputFile = "temp_recording.txt";

const isMac = Deno.build.os === "darwin";

const OPENAI_API_KEY = Deno.env.get("OPENAI_API_KEY");

if (!OPENAI_API_KEY) {
  throw new Error("OPENAI_API_KEY is not set");
}

// Load configurations
const defaultConfig = JSON.parse(
  await Deno.readTextFile("config.json")
) as Config;
let userConfig: Partial<Config> = {};
try {
  userConfig = JSON.parse(
    await Deno.readTextFile("config.user.json")
  ) as Partial<Config>;
} catch {
  // User config is optional
}

// Merge configurations, with user config taking precedence
const config = {
  commonWords: [
    ...defaultConfig.commonWords,
    ...(userConfig.commonWords || []),
  ],
  instructions: {
    ...defaultConfig.instructions,
    ...userConfig.instructions,
    tips: [
      ...defaultConfig.instructions.tips,
      ...(userConfig.instructions?.tips || []),
    ],
  },
};

// OS-specific command configurations
const commands = {
  record: isMac
    ? {
        cmd: "rec",
        args: [audioFile, "rate", "16k", "channels", "1"],
      }
    : {
        cmd: "arecord",
        args: ["-f", "S16_LE", "-c", "1", "-r", "16000", audioFile],
      },
  mute: isMac
    ? {
        cmd: "osascript",
        args: ["-e", "set volume with output muted"],
      }
    : {
        cmd: "pactl",
        args: ["set-sink-mute", "@DEFAULT_SINK@", "1"],
      },
  unmute: isMac
    ? {
        cmd: "osascript",
        args: ["-e", "set volume without output muted"],
      }
    : {
        cmd: "pactl",
        args: ["set-sink-mute", "@DEFAULT_SINK@", "0"],
      },
  type: isMac
    ? {
        cmd: "osascript",
        args: (text: string) => [
          "-e",
          `tell application "System Events" to keystroke "${text.replace(
            /"/g,
            '\\"'
          )}"`,
        ],
      }
    : {
        cmd: "xdotool",
        args: (text: string) => ["type", text],
      },
};

let recordingProcess: Deno.ChildProcess | null = null;
const recordCommand = new Deno.Command(commands.record.cmd, {
  args: commands.record.args,
});

const muteAudio = async (): Promise<void> => {
  try {
    const muteCommand = new Deno.Command(commands.mute.cmd, {
      args: commands.mute.args,
    });
    await muteCommand.output();
  } catch (error) {
    console.error("Error muting audio:", error);
  }
};

const unmuteAudio = async (): Promise<void> => {
  try {
    const unmuteCommand = new Deno.Command(commands.unmute.cmd, {
      args: commands.unmute.args,
    });
    await unmuteCommand.output();
  } catch (error) {
    console.error("Error unmuting audio:", error);
  }
};

let startAt: number | null = null;
let shouldCancelTyping = { value: false };

const startRecording = async (): Promise<void> => {
  console.log("Started recording...");
  await muteAudio();
  recordingProcess = recordCommand.spawn();
  startAt = Date.now();
};

const stopRecording = async (): Promise<void> => {
  if (!startAt) {
    console.error("startAt is null");
    return;
  }
  console.log(
    `Stopped recording after ${((Date.now() - startAt) / 1000).toFixed(
      2
    )} seconds`
  );

  try {
    recordingProcess?.kill();
  } catch {
    // ignore
  }
  recordingProcess = null;
  await unmuteAudio();

  // Reset cancel flag
  shouldCancelTyping.value = false;

  // Wait a bit for the file to be written
  await new Promise((resolve) => setTimeout(resolve, 100));

  // Transcribe
  const text = await (async () => {
    try {
      const transcribeCommand = new Deno.Command("whisper", {
        args: [
          audioFile,
          "--model",
          "base",
          "--language",
          "en",
          "--output_format",
          "txt",
          "--initial_prompt",
          `This is a voice memo. The user is a software developer and manager. Some common words are ${config.commonWords.join(
            ", "
          )}. The memo could be empty. Start of memo:`,
        ],
      });

      await transcribeCommand.output();
      await Deno.remove(audioFile).catch((error) => {
        console.error("Error removing audio file", error);
      });
      const result = await Deno.readTextFile(outputFile).catch((error) => {
        console.error("Error reading output file", error);
        return "";
      });
      await Deno.remove(outputFile).catch((error) => {
        console.error("Error removing output file", error);
      });
      return result.trim();
    } catch (error) {
      console.error("Transcription error:", error);
      return "";
    }
  })();

  if (shouldCancelTyping.value) {
    console.log("Typing cancelled by user");
    return;
  }

  console.log(
    `finished transcribing text after ${((Date.now() - startAt) / 1000).toFixed(
      2
    )} seconds: ${text} at ${(
      (text.split(/\s+/).length / ((Date.now() - startAt) / 1000)) *
      60
    ).toFixed(2)} wpm`
  );

  if (text.trim().length < 10) {
    console.log("Skipping transcription, too short");
    return;
  }

  // clean up the text with an LLM
  const cleanedText = await (async () => {
    const response = await fetch("https://api.openai.com/v1/chat/completions", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${OPENAI_API_KEY}`,
      },
      body: JSON.stringify({
        model: config.instructions.model,
        messages: [
          {
            role: "system",
            content: `
You are a copyeditor. Read the following rough voice transcription and return a copyedited and formatted text.

Tips:
${config.instructions.tips.map((tip: string) => `- ${tip}`).join("\n")}

Pay close attention to the following commonly used words: ${config.commonWords.join(
              ", "
            )}. Prefer these spellings over others.

At the end of the transcription, the author may add a note to you, the copyeditor. This note will start with "Note to the editor". Take this note into account when copyediting as instructions for how to rewrite or reformat the text.

Return the rewritten text in a json object with a key "text" and an optional "error". e.g. {"text": "...", "error": "..."}
`,
          },
          {
            role: "user",
            content: `Here's the transcription:\n\n\`\`\`${
              text || "no transcription provided. just return an empty string."
            }\`\`\``,
          },
        ],
        temperature: config.instructions.temperature,
        response_format: { type: "json_object" },
      }),
    });
    const data = await response.json();

    if (!data.choices) {
      console.error("No choices returned from OpenAI", data);
      return "";
    }

    return JSON.parse(data.choices[0].message.content).text;
  })();

  if (shouldCancelTyping.value) {
    console.log("Typing cancelled by user");
    return;
  }

  console.log(
    `finished cleaning text after ${((Date.now() - startAt) / 1000).toFixed(
      2
    )} seconds: ${cleanedText}`
  );

  // Type the text
  const typeCommand = new Deno.Command(commands.type.cmd, {
    args: commands.type.args(cleanedText.replace(/-/g, "")),
  });
  typeCommand.spawn();
  console.log(
    `finished typing text after ${((Date.now() - startAt) / 1000).toFixed(
      2
    )} seconds at ${(
      (cleanedText.split(/\s+/).length / ((Date.now() - startAt) / 1000)) *
      60
    ).toFixed(2)} wpm`
  );
};

// Learn more at https://docs.deno.com/runtime/manual/examples/module_metadata#concepts
if (import.meta.main) {
  console.log("Starting key listener...");
  if (isMac) {
    await import("./mac.ts").then((mod) =>
      mod.setupMacKeyListener(startRecording, stopRecording, shouldCancelTyping)
    );
  } else {
    await import("./linux.ts").then((mod) =>
      mod.setupLinuxKeyListener(
        startRecording,
        stopRecording,
        shouldCancelTyping
      )
    );
  }
}
