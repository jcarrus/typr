interface KeyEvent {
  isActive?: boolean;
  type?: "escape";
}

export const setupMacKeyListener = async (
  startRecording: () => Promise<void>,
  stopRecording: () => Promise<void>,
  shouldCancelTyping: { value: boolean }
): Promise<void> => {
  const handler = async (request: Request): Promise<Response> => {
    console.log("Received request:", request);
    if (request.method !== "POST") {
      return new Response("Method not allowed", { status: 405 });
    }

    try {
      const event = (await request.json()) as KeyEvent;
      console.log("Received event:", event);

      if (event.type === "escape") {
        shouldCancelTyping.value = true;
      } else if (event.isActive !== undefined) {
        if (event.isActive) {
          await startRecording();
        } else {
          await stopRecording();
        }
      }

      return new Response("OK", { status: 200 });
    } catch (error) {
      console.error("Error processing request:", error);
      return new Response("Bad Request", { status: 400 });
    }
  };

  // Start the server
  console.log("Starting HTTP server on port 3433...");
  const server = Deno.serve({ port: 3433, handler });
  await server.finished;
};
