# Typr

Private, local dictation for macOS. Hold Fn/Globe, speak, then release to
transcribe, clean up, and insert the text at the cursor.

## How it works

- Moonshine Medium Streaming stays loaded and transcribes Core Audio buffers
  while you speak. WhisperKit `small.en_217MB` starts at release as the
  independent reference path.
- Qwen 3.5 4B copyedits both transcripts locally through Ollama. Profile rules
  are passed verbatim as bullets alongside focused Accessibility text, while a
  JSON Schema constrains the response to one insert-ready text field. Typr
  records the exact request, raw response, and timings for each path.
- macOS Accessibility context helps resolve vocabulary, capitalization, and
  writing style from the focused app, window, control, and nearby text.
- Text insertion tries a verified Accessibility write, then invokes the focused
  app's Accessibility Paste command, then falls back to Cmd-V and synthesized
  typing.
- The native app records lossless Core Audio buffers directly, avoiding
  FFmpeg's glitch-prone AVFoundation microphone adapter.
- Every run keeps its native source, AAC listening copy, normalized audio,
  transcription audio, capture diagnostics, raw transcript, typed output,
  models, timings, and insertion method in a private local history.

Audio, application context, prompts, transcripts, and rewritten text stay on
the Mac. Each run archives the exact model request and Accessibility context so
results can be inspected and reproduced.

## Install

```bash
brew install deno ffmpeg whisperkit-cli ollama pipx
pipx install moonshine-voice
moonshine download --language en --model-arch 5 --stt
ollama pull qwen3.5:4b
ollama pull qwen3.6:latest
./build.sh
typr listen
```

`./build.sh` installs `/Applications/Typr.app` and links the CLI at
`~/.local/bin/typr`.

On first launch, enable Typr under **System Settings → Privacy & Security →
Accessibility**, then launch it again. macOS separately asks for Microphone
access on the first recording. The menu-bar waveform means Typr is listening.

Builds require a stable code-signing identity so macOS retains Accessibility
and microphone grants across iterations. `build.sh` uses the local
`Typr Local Development` identity by default, or `TYPR_CODESIGN_IDENTITY` when
set. Create either a self-signed Code Signing certificate in Keychain Access or
an Apple Development certificate before building on another Mac.

## Use

Hold Fn/Globe to record. The menu-bar icon turns green immediately while
recording, yellow while processing, and returns to its normal color when done.
Typr loads Moonshine in-process and keeps Qwen resident in Ollama while the app
runs. Their first launch can take longer while models download and load.

Click the menu-bar icon and choose a result under **Recent Dictations** to copy
it. Choose **Dictation History…** to compare the untouched
Core Audio recording, its AAC listening copy, a conservatively normalized
version, and the 16 kHz audio sent to WhisperKit. Each new run also exposes
buffer-continuity diagnostics. You can compare raw transcripts with typed
output, copy the request ID in one click, and run a transcript through another
local Ollama model. Runs are kept under `~/Library/Application Support/Typr/Runs/`
until you delete them.

Hold Shift+Fn in either order to teach Typr about Justin, vocabulary,
recognition corrections, or writing preferences. Qwen applies atomic add,
replace, and remove operations to the local versioned profile; feedback is
never inserted. Profile editing uses the larger `qwen3.6:latest` model because
these infrequent durable changes favor accuracy over hot-path latency. Typr
unloads its 23 GB weights immediately after each profile update while keeping
the 3 GB dictation model resident for low-latency use. Choose
**Test Shift+Fn…** from the menu to verify the modifier
combination without recording or changing the profile. Profile commands use
schema-constrained JSON and retain the previous dictation only to resolve
explicit references such as “that name.” Rules are short declarative facts such
as “Justin works with Mollie Breen,” not spelling instructions or correction
procedures. Profile updates always notify with the operations that were applied.
Choose **Profile…** from the menu-bar icon to view the effective rules.

Typr inserts the first transcription path as soon as its Qwen rewrite finishes.
The reference path continues independently; the detailed completion
notification includes transcription, prompt, net generation, throughput, and
stop-to-type timings.

Enable **Developer Overlay** in the menu to replace the system notification
with a persistent, non-focusing panel containing the raw dictation, inserted
text, model timings, exact Accessibility context, and reference path. It stays
visible until closed or replaced by the next dictation. **Copy Diagnostics**
copies the complete payload for debugging.
Toggle this with **Completion Notifications** in the menu-bar menu.

Commands:

```bash
typr listen   # Launch the menu-bar app
typr config   # Show the effective configuration
typr warmup   # Start and preload both local models
typr runs     # List the 20 most recent runs
typr review latest
typr rewrite latest --model qwen3.5:4b
typr rewrite latest --current # replay with today's prompt and saved context
typr profile-preview latest # replay a recording without changing the profile
```

## Configure

Typr creates `~/.typr-settings.json`. The useful settings are:

```json
{
  "userProfile": "Justin is a software engineer and cofounder of OverAI, formerly Perygee...",
  "customTerms": ["OverAI", "Perygee", "Abhijay", "Codex", "Ghostty"],
  "whisperPrompt": "Justin is dictating text about OverAI and enterprise internal applications.",
  "whisperKitModel": "small.en_217MB",
  "qwenModel": "qwen3.5:4b",
  "profileModel": "qwen3.6:latest",
  "showCompletionNotification": true
}
```

The first launch migrates `userProfile` and `customTerms` into atomic profile
rules under `~/Library/Application Support/Typr/Profile/`. Accessibility
context and those rules guide rewriting. Older OpenAI settings remain readable
but are ignored; Typr makes no OpenAI requests.

## Troubleshooting

- Check `~/.typr-log.txt` for model startup and per-stage timings.
- Run `typr warmup`, then `ollama ps` to verify Qwen is resident. Moonshine is
  loaded inside the Typr app rather than exposed through a model server.
- If Fn does nothing, relaunch Typr and re-enable its Accessibility permission.
- If recording fails, enable Typr under **Privacy & Security → Microphone**.

## Development

```bash
deno fmt --check typr.ts
deno check typr.ts
./build.sh
```

Typr combines a native Swift menu-bar app for Fn monitoring, Accessibility
context, and insertion with a compiled Deno engine for recording and local
inference.
