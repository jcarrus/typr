import { KeyConfig } from "./types.ts";

export const getLinuxKeyConfig = (): KeyConfig => ({
  command: new Deno.Command("xinput", {
    args: ["test-xi2", "--root"],
    stdout: "piped",
  }),
  isShiftKey: (output: string) =>
    output.includes("detail: 50") || output.includes("detail: 62"),
  isKeyDown: (output: string) => output.includes("RawKeyPress"),
  isKeyUp: (output: string) => output.includes("RawKeyRelease"),
  isEscapeKey: (output: string) =>
    output.includes("detail: 9") && output.includes("RawKeyPress"),
});

export const setupLinuxKeyListener = async (
  startRecording: () => Promise<void>,
  stopRecording: () => Promise<void>,
  shouldCancelTyping: { value: boolean }
): Promise<void> => {
  const keyConfig = getLinuxKeyConfig();
  const process = keyConfig.command.spawn();

  // Cleanup on process exit
  Deno.addSignalListener("SIGINT", () => {
    console.log("SIGINT received, killing process");
    process.kill();
  });

  const reader = process.stdout.getReader();
  let lastShiftPress = 0;
  let isHolding = false;
  let lastKeyUpWasShift = false;

  while (true) {
    const { value, done } = await reader.read();
    if (done) return;

    const output = new TextDecoder().decode(value);
    const now = Date.now();

    if (keyConfig.isShiftKey(output)) {
      if (keyConfig.isKeyDown(output)) {
        if (now - lastShiftPress < 300 && lastKeyUpWasShift) {
          isHolding = true;
          await startRecording();
        }
        lastShiftPress = now;
      } else if (keyConfig.isKeyUp(output)) {
        if (isHolding) {
          isHolding = false;
          await stopRecording();
        } else if (!isHolding) {
          lastKeyUpWasShift = true;
        }
      }
    } else if (keyConfig.isEscapeKey(output) && !isHolding) {
      shouldCancelTyping.value = true;
    } else {
      lastKeyUpWasShift = false;
    }
  }
};
