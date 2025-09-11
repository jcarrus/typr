#!/usr/bin/env -S deno run --allow-all

import { parseArgs } from "jsr:@std/cli";
import { exists } from "jsr:@std/fs";
import { join } from "jsr:@std/path";
import { z } from "npm:zod";

// Default prompts
const DEFAULT_WHISPER_PROMPT =
  "The following is a transcription of a dictation from a speaker who is XXX. The speaker sometimes discusses the following topics: YYY. The speaker sometimes uses the following uncommon terms: ZZZ.";

const DEFAULT_LLM_PROMPT =
  "You are a helpful assistant that will carefully examine the following transcription of a dictation and then carefully make the modifications requested of the editor.";

// Types and interfaces
const settingsSchema = z.object({
  openAIKey: z.string().default(""),
  whisperPrompt: z.string().default(DEFAULT_WHISPER_PROMPT),
  llmPrompt: z.string().default(DEFAULT_LLM_PROMPT),
  useLocalWhisper: z.boolean().default(false),
});

type Settings = z.infer<typeof settingsSchema>;

// Logging system
const LOG_FILE = join(Deno.env.get("HOME") || ".", ".typr-log.txt");

async function logToFile(
  level: "INFO" | "ERROR",
  message: string,
  error?: unknown
): Promise<void> {
  const timestamp = new Date().toISOString();
  const logMessage = `[${timestamp}] ${level}: ${message}${
    error ? ` - ${error}` : ""
  }\n`;
  try {
    await Deno.writeTextFile(LOG_FILE, logMessage, { append: true });
  } catch (e) {
    // Fallback to console if logging fails
    console.error("Failed to write to log file:", e);
  }
}

// Settings management
const SETTINGS_FILE = join(Deno.env.get("HOME") || ".", ".typr-settings.json");
const STATE_FILE = join(Deno.env.get("HOME") || ".", ".typr-state.json");

async function loadSettings(): Promise<Settings> {
  // If the file doesn't exist, create it
  if (!(await exists(SETTINGS_FILE))) {
    await Deno.writeTextFile(
      SETTINGS_FILE,
      JSON.stringify(settingsSchema.parse({}), null, 2)
    );
  }

  // Load the settings
  const data = await Deno.readTextFile(SETTINGS_FILE);
  return settingsSchema.parse(JSON.parse(data));
}

// Get and set state
async function getState(key: string): Promise<string | null> {
  if (!(await exists(STATE_FILE))) {
    return null;
  }
  const data = await Deno.readTextFile(STATE_FILE);
  try {
    return JSON.parse(data)[key];
  } catch (error) {
    await logToFile("ERROR", "Failed to get state", error);
    await clearState();
    return null;
  }
}

async function setState(key: string, value: string): Promise<void> {
  const data = await Deno.readTextFile(STATE_FILE);
  const state = JSON.parse(data);
  state[key] = value;
  await Deno.writeTextFile(STATE_FILE, JSON.stringify(state, null, 2));
}

async function clearState(): Promise<void> {
  await Deno.writeTextFile(STATE_FILE, JSON.stringify({}, null, 2));
}

// Notification functions
async function notify(
  message: string,
  urgency: "low" | "normal" | "critical" = "normal"
): Promise<void> {
  try {
    // Use desktop notifications - works across all modern Linux desktops
    const command = new Deno.Command("notify-send", {
      args: ["--urgency", urgency, "--app-name", "Typr", message],
    });
    await command.output();
  } catch (error) {
    // Fallback to console if notifications fail
    console.log(`🔔 ${message}`);
    await logToFile("ERROR", "Failed to send notification", error);
  }
}

// Audio feedback functions
async function playBeep(): Promise<void> {
  try {
    // Use system bell - simple, universal, and reliable
    const command = new Deno.Command("printf", { args: ["\\a"] });
    await command.output();
  } catch (error) {
    // Silent fallback - don't let beep failures break recording
    await logToFile("ERROR", "Failed to play system bell", error);
  }
}

async function playDoubleBeep(): Promise<void> {
  await playBeep();
  // Short pause between beeps
  await new Promise((resolve) => setTimeout(resolve, 150));
  await playBeep();
}

// Audio recording functions
async function isWhisperAvailable(): Promise<boolean> {
  try {
    const command = new Deno.Command("whisper", { args: ["--help"] });
    const process = command.spawn();
    const status = await process.status;
    return status.success;
  } catch {
    return false;
  }
}

async function startRecording(): Promise<string | null> {
  setState("isRecording", "true");

  // Clean up old recordings to prevent /tmp from filling up
  try {
    const command = new Deno.Command("find", {
      args: [
        "/tmp",
        "-name",
        "typr-recording-*.wav",
        "-mtime",
        "+1",
        "-delete",
      ],
    });
    await command.output();
  } catch (e) {
    logToFile("ERROR", "Failed to clean up old recordings", e);
  }

  const audioPath = `/tmp/typr-recording-${new Date()
    .toISOString()
    .replace(/[:.]/g, "-")}.wav`;

  logToFile("INFO", `Recording to ${audioPath}`);
  setState("audioPath", audioPath);

  // Use ffmpeg for cross-platform audio recording
  const command = new Deno.Command("ffmpeg", {
    args: [
      "-f",
      "pulse",
      "-i",
      "default",
      "-acodec",
      "pcm_s16le",
      "-ar",
      "16000",
      "-ac",
      "1",
      "-y", // Overwrite output file
      audioPath,
    ],
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });

  const process = command.spawn();

  // Log and save the ffmpeg process PID for the second process to kill
  logToFile("INFO", `FFmpeg process PID: ${process.pid}`);
  await setState("ffmpegPid", process.pid.toString());

  await playBeep(); // System bell for start
  await notify("🎙️ Recording started", "low");
  await logToFile("INFO", "🎙️  Recording started...");

  const startTimestamp = Date.now();

  // Wait for the ffmpeg process to finish (either killed by second process or naturally)
  const status = await process.status;
  await logToFile("INFO", `FFmpeg process finished with code: ${status.code}`);

  // If it's been less than 1 second, then just exit
  if (Date.now() - startTimestamp < 1000) {
    await logToFile("INFO", "Recording stopped after less than 1 second");
    await notify("⏹️ Recording stopped after less than 1 second", "low");
    return null;
  }

  return audioPath;
}

// Transcription functions
async function transcribeWithLocalWhisper(
  audioPath: string,
  whisperPrompt: string
): Promise<string> {
  await logToFile(
    "INFO",
    `🔄 Using local Whisper for transcription... ${audioPath}`
  );

  const args = [
    audioPath,
    "--model",
    "base",
    "--language",
    "en",
    "--output_format",
    "txt",
    "--output_dir",
    "/tmp",
    "--verbose",
    "False",
  ];

  if (whisperPrompt.trim()) {
    args.push("--initial_prompt", whisperPrompt);
  }

  const command = new Deno.Command("whisper", { args });
  const process = command.spawn();
  const status = await process.status;

  if (!status.success) {
    throw new Error("Whisper transcription failed");
  }

  // Read the output file
  const baseName =
    audioPath
      .split("/")
      .pop()
      ?.replace(/\.[^/.]+$/, "") || "recording";
  const outputFile = `/tmp/${baseName}.txt`;

  try {
    const content = await Deno.readTextFile(outputFile);
    await Deno.remove(outputFile); // Clean up
    return content.trim();
  } catch (error) {
    throw new Error(`Failed to read transcription output: ${error}`);
  }
}

async function transcribeWithOpenAI(
  audioPath: string,
  apiKey: string,
  whisperPrompt: string
): Promise<string> {
  await logToFile("INFO", "🔄 Using OpenAI API for transcription...");

  const audioData = await Deno.readFile(audioPath);

  const formData = new FormData();
  formData.append("file", new Blob([audioData]), "audio.wav");
  formData.append("model", "whisper-1");
  formData.append("response_format", "text");
  formData.append("language", "en");
  formData.append("temperature", "0.2");

  if (whisperPrompt.trim()) {
    formData.append("prompt", whisperPrompt + "\n\nTranscription:");
  }

  const response = await fetch(
    "https://api.openai.com/v1/audio/transcriptions",
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${apiKey}`,
      },
      body: formData,
    }
  );

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`OpenAI API error: ${errorText}`);
  }

  return await response.text();
}

// Text processing with OpenAI
async function processWithGPT(
  transcription: string,
  apiKey: string,
  llmPrompt: string
): Promise<string> {
  await logToFile("INFO", "🤖 Processing with GPT-4o-mini...");

  const response = await fetch("https://api.openai.com/v1/chat/completions", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      model: "gpt-4o-mini",
      messages: [
        {
          role: "user",
          content: `Task: ${llmPrompt}\n\nTranscription: ${transcription}`,
        },
      ],
      temperature: 0.2,
    }),
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`OpenAI API error: ${errorText}`);
  }

  const data = await response.json();
  return data.choices[0]?.message?.content?.trim() || transcription;
}

// Text typing simulation
async function typeText(text: string): Promise<void> {
  await logToFile("INFO", `⌨️  Typing ${text.length} characters...`);

  try {
    switch (Deno.build.os) {
      case "darwin":
        await typeMacOS(text);
        break;
      case "linux":
        await typeLinux(text);
        break;
      default:
        throw new Error(
          `Unsupported OS: ${Deno.build.os}. Only Linux and macOS are supported.`
        );
    }
  } catch (error) {
    await logToFile("ERROR", "Failed to type text", error);
  }
}

async function typeMacOS(text: string): Promise<void> {
  const script = `tell application "System Events" to keystroke "${text.replace(
    /"/g,
    '\\"'
  )}"`;
  const command = new Deno.Command("osascript", { args: ["-e", script] });
  await command.output();
}

async function typeLinux(text: string): Promise<void> {
  // Use xdotool for Linux
  const command = new Deno.Command("xdotool", { args: ["type", text] });
  await command.output();
}

// Audio processing pipeline
async function processAudioFile(
  audioPath: string
): Promise<{ transcription: string; openaiResponse: string }> {
  const config = await loadSettings();

  if (!config.openAIKey && !config.useLocalWhisper) {
    await logToFile(
      "ERROR",
      "No OpenAI API key configured and local Whisper not enabled"
    );
    throw new Error(
      "No OpenAI API key configured and local Whisper not enabled"
    );
  }

  let transcription = "";

  // Try local Whisper first if enabled
  if (config.useLocalWhisper && (await isWhisperAvailable())) {
    try {
      transcription = await transcribeWithLocalWhisper(
        audioPath,
        config.whisperPrompt
      );
    } catch (error) {
      await logToFile(
        "ERROR",
        "Local Whisper failed, falling back to OpenAI",
        error
      );
    }
  }

  // Use OpenAI if local Whisper wasn't used or failed
  if (!transcription && config.openAIKey) {
    transcription = await transcribeWithOpenAI(
      audioPath,
      config.openAIKey,
      config.whisperPrompt
    );
  }

  if (!transcription || transcription.length < 10) {
    throw new Error("Transcription failed or too short");
  }

  // Apply simple replacements
  transcription = transcription.replace(/slap/gi, "\n");

  // Clean up whitespace
  transcription = transcription
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .join("\n");

  let openaiResponse = transcription;

  // Process with GPT if "note to the editor" is mentioned
  if (config.openAIKey) {
    try {
      openaiResponse = await processWithGPT(
        transcription,
        config.openAIKey,
        config.llmPrompt
      );
    } catch (error) {
      await logToFile("ERROR", "GPT processing failed", error);
    }
  }

  return { transcription, openaiResponse };
}

// Global shortcut setup instructions
function showShortcutInstructions(): void {
  const scriptPath = new URL(import.meta.url).pathname;
  const command = `${Deno.execPath()} run --allow-all "${scriptPath}" toggle`;

  const platformInstructions =
    Deno.build.os === "linux"
      ? `🐧 Linux Setup:
1. For i3: Add to ~/.config/i3/config:
   bindsym --release Mod4+Shift+space exec "${command}"
   (Note: --release flag prevents multiple fires)
   Then run: i3-msg reload

2. For GNOME: Settings > Keyboard > Custom Shortcuts
   - Name: Typr Toggle
   - Command: ${command}
   - Shortcut: Super+Shift+Space
   (GNOME typically fires on key release by default)

3. For KDE: System Settings > Shortcuts > Custom Shortcuts`
      : Deno.build.os === "darwin"
      ? `🍎 macOS Setup:
1. Install a shortcut manager like Karabiner-Elements or BetterTouchTool
2. Or use Automator + System Preferences:
   - Create new 'Quick Action' in Automator
   - Add 'Run Shell Script' action
   - Script: ${command}
   - Save as 'Typr Toggle'
   - System Preferences > Keyboard > Shortcuts > Services
   - Assign ⌘⇧Space to 'Typr Toggle'`
      : "Manual setup required for your OS";

  console.log(`🔗 Global Shortcut Setup
========================

To enable toggle recording, bind this command to a keyboard shortcut:
Command: ${command}

${platformInstructions}

💡 How it works:
   - First press: Starts recording
   - Second press: Stops recording and types the result

📊 Monitor logs in real-time:
   tail -f ${LOG_FILE}    # Watch logs`);
}

async function handleToggleRecording(): Promise<void> {
  // Check if there's already a recording (ffmpeg) process running
  const ffmpegPid = await getState("ffmpegPid");

  if (ffmpegPid) {
    // Second toggle: Kill the ffmpeg process directly
    await notify("⏹️ Stopping recording...", "low");
    await playDoubleBeep();
    try {
      await logToFile("INFO", `Sending SIGTERM to ffmpeg process ${ffmpegPid}`);
      Deno.kill(parseInt(ffmpegPid), "SIGTERM");
    } catch (e) {
      await logToFile("ERROR", "Failed to kill ffmpeg process", e);
    }
    return;
  }

  // First toggle: Start recording process
  await clearState();

  const recordedPath = await startRecording();
  if (!recordedPath) {
    await logToFile("ERROR", "Failed to start recording");
    return;
  }

  // Recording completed (ffmpeg terminated), process the audio
  await clearState(); // Clear the ffmpeg PID
  await logToFile("INFO", "🔄 Processing recorded audio...");
  try {
    const result = await processAudioFile(recordedPath);
    await notify("✅ Typing result...", "low");
    await typeText(result.openaiResponse);
    await notify("🎯 Done!", "low");
  } catch (error) {
    await logToFile("ERROR", "Failed to process audio", error);
    await notify("❌ Processing failed", "critical");
  }
}

// CLI Commands
async function showConfig(): Promise<void> {
  const config = await loadSettings();
  console.log(config);
}

// Main CLI handler
async function main(): Promise<void> {
  const args = parseArgs(Deno.args);
  const command = args._[0] as string;

  switch (command) {
    case "toggle":
      await handleToggleRecording();
      break;
    case "config":
      await showConfig();
      break;
    case "shortcuts":
      showShortcutInstructions();
      break;
    default:
      console.log(`Typr - Elegant dictation with press-and-hold recording

Usage:
  typr config     - Show current configuration
  typr shortcuts  - Show keyboard shortcut setup instructions
  typr toggle     - Toggle recording (used by shortcuts)

Quick Start:
  1. Add your OpenAI key to ~/.typr-settings.json
  2. typr shortcuts   # Setup keyboard shortcut
  3. Use your shortcut to record!

How it works:
  - First press: Starts recording
  - Second press: Stops recording and types the result`);
      break;
  }
}

if (import.meta.main) {
  await main();
}
